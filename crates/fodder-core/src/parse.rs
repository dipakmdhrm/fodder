use crate::models::NewItem;
use crate::{Error, Result};

#[derive(Debug)]
pub struct ParsedFeed {
    pub title: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub items: Vec<NewItem>,
}

pub fn parse_feed(body: &[u8]) -> Result<ParsedFeed> {
    let feed = feed_rs::parser::parse(body).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(ParsedFeed {
        title: feed.title.map(|t| t.content).unwrap_or_default(),
        site_url: feed
            .links
            .iter()
            .find(|l| l.rel.as_deref() != Some("self"))
            .map(|l| l.href.clone()),
        description: feed.description.map(|t| t.content),
        items: feed.entries.into_iter().map(entry_to_item).collect(),
    })
}

fn entry_to_item(entry: feed_rs::model::Entry) -> NewItem {
    let link = entry
        .links
        .iter()
        .find(|l| matches!(l.rel.as_deref(), None | Some("alternate")))
        .or_else(|| entry.links.first())
        .map(|l| l.href.clone());
    let title = entry.title.map(|t| t.content).unwrap_or_default();
    let author = entry
        .authors
        .iter()
        .map(|p| p.name.trim())
        .find(|n| !n.is_empty())
        .map(str::to_string);
    let content_html = entry
        .content
        .and_then(|c| c.body)
        .or_else(|| entry.summary.map(|s| s.content));
    let published_at = entry.published.or(entry.updated).map(|d| d.timestamp());

    // Dedup key. feed-rs fills entry.id deterministically (guid, or a hash of
    // link/title) EXCEPT when an entry has neither guid nor link — then it is
    // a random UUID that would duplicate the item on every repoll. Replace
    // that case with a stable content hash.
    let guid = if entry.id.trim().is_empty() || (link.is_none() && looks_like_bare_uuid(&entry.id))
    {
        let mut basis = title.clone();
        if let Some(c) = &content_html {
            basis.push_str(c);
        }
        format!("fnv:{:016x}", fnv1a64(basis.as_bytes()))
    } else {
        entry.id
    };

    NewItem {
        guid,
        title,
        link,
        author,
        published_at,
        content_html,
    }
}

fn looks_like_bare_uuid(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

/// FNV-1a: tiny and stable across releases (unlike `DefaultHasher`), which
/// matters because the hash persists in the DB as a dedup key.
fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS2: &str = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Example &amp; Sons</title>
  <link>https://example.org/</link>
  <description>News</description>
  <item>
    <guid>tag:example.org,2026:1</guid>
    <title>First post</title>
    <link>https://example.org/1</link>
    <author>alice@example.org (Alice)</author>
    <pubDate>Wed, 01 Jul 2026 12:00:00 GMT</pubDate>
    <description><![CDATA[<p>Hello <b>world</b></p>]]></description>
  </item>
  <item>
    <title>No guid, has link</title>
    <link>https://example.org/2</link>
  </item>
</channel></rss>"#;

    const ATOM: &str = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Feed</title>
  <link rel="self" href="https://example.org/atom.xml"/>
  <link rel="alternate" href="https://example.org/"/>
  <updated>2026-07-01T12:00:00Z</updated>
  <id>urn:uuid:feed</id>
  <entry>
    <id>urn:uuid:entry-1</id>
    <title>Entry one</title>
    <link href="https://example.org/e1"/>
    <updated>2026-07-01T12:00:00Z</updated>
    <content type="html">&lt;p&gt;body&lt;/p&gt;</content>
  </entry>
</feed>"#;

    #[test]
    fn parses_rss2_with_entities() {
        let feed = parse_feed(RSS2.as_bytes()).unwrap();
        assert_eq!(feed.title, "Example & Sons");
        assert_eq!(feed.site_url.as_deref(), Some("https://example.org/"));
        assert_eq!(feed.items.len(), 2);
        let first = &feed.items[0];
        assert_eq!(first.guid, "tag:example.org,2026:1");
        assert_eq!(first.title, "First post");
        assert!(first
            .content_html
            .as_deref()
            .unwrap()
            .contains("<b>world</b>"));
        assert!(first.published_at.is_some());
    }

    // feed-rs synthesizes a deterministic id when an entry lacks a guid; what
    // matters for dedup is that the id is non-empty and stable across parses.
    #[test]
    fn missing_guid_gets_deterministic_id() {
        let a = parse_feed(RSS2.as_bytes()).unwrap();
        let b = parse_feed(RSS2.as_bytes()).unwrap();
        let second = &a.items[1];
        assert!(!second.guid.trim().is_empty());
        assert_eq!(second.guid, b.items[1].guid);
        assert_eq!(second.published_at, None);
    }

    // Without guid AND link, feed-rs invents a random UUID per parse; our
    // content-hash fallback must kick in or repolls would duplicate items.
    #[test]
    fn missing_guid_and_link_falls_back_to_stable_hash() {
        let rss = r#"<?xml version="1.0"?><rss version="2.0"><channel>
            <title>t</title><item><title>only a title</title></item>
        </channel></rss>"#;
        let a = parse_feed(rss.as_bytes()).unwrap();
        let b = parse_feed(rss.as_bytes()).unwrap();
        assert!(a.items[0].guid.starts_with("fnv:"));
        assert_eq!(a.items[0].guid, b.items[0].guid);
    }

    #[test]
    fn parses_atom_and_skips_self_link_for_site_url() {
        let feed = parse_feed(ATOM.as_bytes()).unwrap();
        assert_eq!(feed.title, "Atom Feed");
        assert_eq!(feed.site_url.as_deref(), Some("https://example.org/"));
        let entry = &feed.items[0];
        assert_eq!(entry.guid, "urn:uuid:entry-1");
        assert_eq!(entry.content_html.as_deref(), Some("<p>body</p>"));
        // Atom updated used when published is absent
        assert!(entry.published_at.is_some());
    }

    #[test]
    fn non_feed_content_is_an_error() {
        assert!(matches!(
            parse_feed(b"<!DOCTYPE html><html><body>nope</body></html>"),
            Err(Error::Parse(_))
        ));
    }
}
