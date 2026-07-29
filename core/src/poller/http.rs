//! Low-level conditional GET: sends validators, classifies the response.
//!
//! Kept separate from parsing so the network/status handling can be tested with
//! a mock server while feed parsing is tested with static fixture bytes.

use std::time::Duration;

use chrono::Utc;
use reqwest::header::{
    HeaderValue, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED, RETRY_AFTER,
};
use reqwest::{Client, StatusCode};

/// Outcome of a single conditional GET, before any feed parsing.
#[derive(Debug)]
pub enum FetchResponse {
    /// Server returned 304 — our cached copy is current.
    NotModified,
    /// Server returned a body. Carries the fresh validators to store.
    Modified {
        etag: Option<String>,
        last_modified: Option<String>,
        body: Vec<u8>,
    },
    /// Server asked us to back off (429, or 503 with Retry-After).
    RateLimited { retry_after: Duration },
    /// Transport error or non-success status.
    Error(String),
}

/// Perform a conditional GET for `url`, replaying stored `etag` / `last_modified`
/// as `If-None-Match` / `If-Modified-Since`.
pub async fn conditional_get(
    client: &Client,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> FetchResponse {
    let mut req = client.get(url);
    if let Some(tag) = etag {
        if let Ok(v) = HeaderValue::from_str(tag) {
            req = req.header(IF_NONE_MATCH, v);
        }
    }
    if let Some(lm) = last_modified {
        if let Ok(v) = HeaderValue::from_str(lm) {
            req = req.header(IF_MODIFIED_SINCE, v);
        }
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return FetchResponse::Error(format!("request failed: {e}")),
    };

    let status = resp.status();

    if status == StatusCode::NOT_MODIFIED {
        return FetchResponse::NotModified;
    }

    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after)
            // Default backoff when the server rate-limits without a hint.
            .unwrap_or_else(|| Duration::from_secs(300));
        return FetchResponse::RateLimited { retry_after };
    }

    if !status.is_success() {
        return FetchResponse::Error(format!("HTTP {status}"));
    }

    let header_str = |name: reqwest::header::HeaderName| {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    let etag = header_str(ETAG);
    let last_modified = header_str(LAST_MODIFIED);

    match resp.bytes().await {
        Ok(bytes) => FetchResponse::Modified {
            etag,
            last_modified,
            body: bytes.to_vec(),
        },
        Err(e) => FetchResponse::Error(format!("reading body: {e}")),
    }
}

/// Parse a `Retry-After` header value, which is either an integer number of
/// seconds or an HTTP-date (IMF-fixdate). Returns the delay from now.
pub fn parse_retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();

    // Form 1: delta-seconds.
    if let Ok(secs) = value.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }

    // Form 2: HTTP-date, e.g. "Wed, 21 Oct 2025 07:28:00 GMT".
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(value, "%a, %d %b %Y %H:%M:%S GMT") {
        let target = naive.and_utc();
        let delta = target.signed_duration_since(Utc::now());
        return Some(delta.to_std().unwrap_or(Duration::ZERO));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_seconds() {
        assert_eq!(parse_retry_after("120"), Some(Duration::from_secs(120)));
        assert_eq!(parse_retry_after("  0 "), Some(Duration::ZERO));
    }

    #[test]
    fn retry_after_http_date() {
        // Far-future date should yield a large positive delay.
        let d = parse_retry_after("Wed, 21 Oct 2099 07:28:00 GMT").unwrap();
        assert!(d.as_secs() > 0);
    }

    #[test]
    fn retry_after_garbage_is_none() {
        assert_eq!(parse_retry_after("soon"), None);
    }
}
