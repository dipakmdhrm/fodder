//! The feed poller: a shared HTTP client, conditional GET, parsing, dedupe, and
//! bounded-concurrency scheduling with per-feed exponential backoff.

pub mod dedupe;
pub mod http;

use std::time::Duration;

use futures::stream::{self, StreamExt};
use reqwest::Client;

use crate::models::{Feed, NewArticle};

pub use http::FetchResponse;

/// The result of polling one feed.
#[derive(Debug)]
pub enum PollOutcome {
    /// 304 — nothing changed; just reschedule.
    NotModified,
    /// New content parsed. Carries fresh validators and the parsed items
    /// (already deduped by GUID at the DB layer on insert).
    Updated {
        title: Option<String>,
        etag: Option<String>,
        last_modified: Option<String>,
        items: Vec<NewArticle>,
    },
    /// Server rate-limited us; retry no sooner than `retry_after`.
    RateLimited { retry_after: Duration },
    /// Transport, status, or parse error.
    Error(String),
}

/// Owns the shared `reqwest` client and the concurrency limit.
pub struct Poller {
    client: Client,
    concurrency: usize,
}

/// The user agent sent with every request.
const USER_AGENT: &str = concat!("FodderReader/", env!("CARGO_PKG_VERSION"));

impl Poller {
    /// Build a poller with a rustls-backed client (gzip, sane timeouts) and the
    /// given max concurrent fetches.
    pub fn new(concurrency: usize) -> Self {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("building reqwest client");
        Self {
            client,
            concurrency: concurrency.max(1),
        }
    }

    /// Access the shared client (reused for feed discovery).
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Poll a single feed: conditional GET, then parse on a fresh body.
    pub async fn poll_feed(&self, feed: &Feed) -> PollOutcome {
        match http::conditional_get(
            &self.client,
            &feed.url,
            feed.etag.as_deref(),
            feed.last_modified.as_deref(),
        )
        .await
        {
            FetchResponse::NotModified => PollOutcome::NotModified,
            FetchResponse::RateLimited { retry_after } => PollOutcome::RateLimited { retry_after },
            FetchResponse::Error(e) => PollOutcome::Error(e),
            FetchResponse::Modified {
                etag,
                last_modified,
                body,
            } => match parse_items(&body) {
                Ok((title, items)) => PollOutcome::Updated {
                    title,
                    etag,
                    last_modified,
                    items,
                },
                Err(e) => PollOutcome::Error(format!("parse: {e}")),
            },
        }
    }

    /// Poll many feeds concurrently, capped at `self.concurrency`. Returns each
    /// feed's id paired with its outcome; order is not preserved.
    pub async fn poll_all(&self, feeds: Vec<Feed>) -> Vec<(i64, PollOutcome)> {
        stream::iter(feeds)
            .map(|feed| async move {
                let id = feed.id;
                (id, self.poll_feed(&feed).await)
            })
            .buffer_unordered(self.concurrency)
            .collect()
            .await
    }
}

/// Parse raw feed bytes into a title and article list. Handles RSS/Atom/JSON
/// Feed via `feed-rs`'s auto-detection.
pub fn parse_items(bytes: &[u8]) -> Result<(Option<String>, Vec<NewArticle>), String> {
    let feed = feed_rs::parser::parse(bytes).map_err(|e| e.to_string())?;

    let title = feed.title.map(|t| t.content);

    let items = feed
        .entries
        .iter()
        .map(|entry| {
            let guid = dedupe::stable_guid(entry);
            let title = entry
                .title
                .as_ref()
                .map(|t| t.content.clone())
                .unwrap_or_default();
            let url = entry.links.first().map(|l| l.href.clone());
            let content = entry
                .content
                .as_ref()
                .and_then(|c| c.body.clone())
                .or_else(|| entry.summary.as_ref().map(|s| s.content.clone()));
            let published = entry.published.or(entry.updated);
            NewArticle {
                guid,
                title,
                url,
                content,
                published,
            }
        })
        .collect();

    Ok((title, items))
}

/// Next backoff delay for a feed after `error_count` consecutive failures.
/// Exponential from `base`, doubling per failure, capped at 6 hours.
pub fn backoff_next(error_count: i32, base: Duration) -> Duration {
    const CAP: Duration = Duration::from_secs(6 * 60 * 60);
    let exp = error_count.clamp(0, 16) as u32;
    let factor = 1u64 << exp.min(20);
    base.checked_mul(factor as u32)
        .map(|d| d.min(CAP))
        .unwrap_or(CAP)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &[u8] = br#"<?xml version="1.0"?>
        <rss version="2.0"><channel>
          <title>Example Feed</title>
          <item><guid>guid-1</guid><title>First</title>
            <link>https://e.com/1</link><description>Body one</description></item>
          <item><guid>guid-2</guid><title>Second</title>
            <link>https://e.com/2</link><description>Body two</description></item>
        </channel></rss>"#;

    #[test]
    fn parses_rss_title_and_items() {
        let (title, items) = parse_items(RSS).unwrap();
        assert_eq!(title.as_deref(), Some("Example Feed"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].guid, "guid-1");
        assert_eq!(items[0].title, "First");
        assert_eq!(items[0].url.as_deref(), Some("https://e.com/1"));
    }

    #[test]
    fn backoff_grows_then_caps() {
        let base = Duration::from_secs(60);
        assert_eq!(backoff_next(0, base), Duration::from_secs(60));
        assert_eq!(backoff_next(1, base), Duration::from_secs(120));
        assert_eq!(backoff_next(3, base), Duration::from_secs(480));
        // Very large error counts saturate at the 6h cap, never overflow.
        assert_eq!(backoff_next(100, base), Duration::from_secs(6 * 60 * 60));
    }
}
