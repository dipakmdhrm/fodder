//! Domain models shared across the store, poller, and UI.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A subscribed feed and its conditional-GET / error bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub id: i64,
    pub url: String,
    pub title: String,
    /// Last `ETag` seen, replayed as `If-None-Match`.
    pub etag: Option<String>,
    /// Last `Last-Modified` seen, replayed as `If-Modified-Since`.
    pub last_modified: Option<String>,
    /// Human-readable text of the most recent poll error, or `None` if healthy.
    pub last_error: Option<String>,
    /// Consecutive error count, drives exponential backoff.
    pub error_count: i32,
    /// Earliest time this feed should be polled again.
    pub next_poll_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// A stored article. `guid` is the stable dedupe key within a feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub id: i64,
    pub feed_id: i64,
    pub guid: String,
    pub title: String,
    pub url: Option<String>,
    pub content: Option<String>,
    pub published: Option<DateTime<Utc>>,
    pub is_read: bool,
    pub seen_at: DateTime<Utc>,
}

/// A freshly-parsed article, before it is assigned a row id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewArticle {
    pub guid: String,
    pub title: String,
    pub url: Option<String>,
    pub content: Option<String>,
    pub published: Option<DateTime<Utc>>,
}
