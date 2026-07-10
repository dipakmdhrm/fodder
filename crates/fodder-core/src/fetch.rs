use std::io::Read;
use std::time::Duration;

use crate::{Error, Result};

pub use ureq::Agent;

const MAX_REDIRECTS: usize = 5;
const MAX_BODY_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug)]
pub enum FetchOutcome {
    /// Server answered 304 — cached content is still current.
    NotModified,
    Fetched(FetchedFeed),
}

#[derive(Debug)]
pub struct FetchedFeed {
    pub body: Vec<u8>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    /// Set when the feed moved through permanent (301/308) redirects only;
    /// the subscription URL should be updated to this.
    pub permanent_url: Option<String>,
}

/// Shared agent so keep-alive connections are reused across a poll run.
pub fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .redirects(0) // handled manually to detect permanent moves
        .user_agent(concat!("fodder/", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn fetch_feed(
    agent: &ureq::Agent,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchOutcome> {
    let mut current = url.to_string();
    let mut all_permanent = true;

    for _ in 0..=MAX_REDIRECTS {
        let mut req = agent.get(&current);
        if let Some(etag) = etag {
            req = req.set("If-None-Match", etag);
        }
        if let Some(lm) = last_modified {
            req = req.set("If-Modified-Since", lm);
        }

        let resp = match req.call() {
            Ok(resp) => resp,
            Err(e) => return Err(Error::Http(e.to_string())),
        };

        match resp.status() {
            200..=299 => {
                let etag = resp.header("ETag").map(str::to_string);
                let last_modified = resp.header("Last-Modified").map(str::to_string);
                let mut body = Vec::new();
                resp.into_reader()
                    .take(MAX_BODY_BYTES)
                    .read_to_end(&mut body)?;
                let permanent_url = (all_permanent && current != url).then_some(current);
                return Ok(FetchOutcome::Fetched(FetchedFeed {
                    body,
                    etag,
                    last_modified,
                    permanent_url,
                }));
            }
            304 => return Ok(FetchOutcome::NotModified),
            status @ (301 | 302 | 303 | 307 | 308) => {
                if !matches!(status, 301 | 308) {
                    all_permanent = false;
                }
                let location = resp
                    .header("Location")
                    .ok_or_else(|| Error::Http(format!("{status} without Location")))?;
                current = url::Url::parse(&current)
                    .and_then(|base| base.join(location))
                    .map_err(|e| Error::Http(format!("bad redirect target: {e}")))?
                    .to_string();
            }
            status => return Err(Error::Http(format!("unexpected status {status}"))),
        }
    }
    Err(Error::Http("too many redirects".into()))
}
