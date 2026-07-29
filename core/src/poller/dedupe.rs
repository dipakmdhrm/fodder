//! Stable per-item identity for deduplication.
//!
//! Prefer the feed-provided id/GUID. When it's absent, synthesize a stable key
//! from a SHA-256 of the item's link and title so the same item hashes the same
//! on every poll and never re-notifies.

use feed_rs::model::Entry;
use sha2::{Digest, Sha256};

/// Compute the dedupe key for a parsed entry.
pub fn stable_guid(entry: &Entry) -> String {
    let id = entry.id.trim();
    if !id.is_empty() {
        return id.to_string();
    }

    let link = entry.links.first().map(|l| l.href.as_str()).unwrap_or("");
    let title = entry
        .title
        .as_ref()
        .map(|t| t.content.as_str())
        .unwrap_or("");
    hash_link_title(link, title)
}

/// SHA-256 of `link` + `\n` + `title`, hex-encoded. Deterministic and stable.
pub fn hash_link_title(link: &str, title: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(link.as_bytes());
    hasher.update(b"\n");
    hasher.update(title.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use feed_rs::model::{Entry, Link, Text};

    fn entry_with(id: &str, link: Option<&str>, title: Option<&str>) -> Entry {
        let mut e = Entry::default();
        e.id = id.to_string();
        if let Some(l) = link {
            e.links = vec![Link {
                href: l.to_string(),
                rel: None,
                media_type: None,
                href_lang: None,
                title: None,
                length: None,
            }];
        }
        if let Some(t) = title {
            e.title = Some(Text {
                content_type: "text/plain".parse().unwrap(),
                src: None,
                content: t.to_string(),
            });
        }
        e
    }

    #[test]
    fn guid_used_when_present() {
        let e = entry_with("urn:uuid:123", Some("https://x/1"), Some("T"));
        assert_eq!(stable_guid(&e), "urn:uuid:123");
    }

    #[test]
    fn hash_fallback_when_id_empty() {
        let e = entry_with("", Some("https://x/1"), Some("Title"));
        let g = stable_guid(&e);
        assert_eq!(g, hash_link_title("https://x/1", "Title"));
        assert_eq!(g.len(), 64, "sha-256 hex is 64 chars");
    }

    #[test]
    fn hash_is_deterministic_and_distinct() {
        assert_eq!(
            hash_link_title("https://x/1", "A"),
            hash_link_title("https://x/1", "A")
        );
        assert_ne!(
            hash_link_title("https://x/1", "A"),
            hash_link_title("https://x/2", "A")
        );
    }

    #[test]
    fn same_item_same_guid_idempotent() {
        let a = entry_with("id-1", Some("https://x/1"), Some("A"));
        let b = entry_with("id-1", Some("https://x/1"), Some("A"));
        assert_eq!(stable_guid(&a), stable_guid(&b));
    }
}
