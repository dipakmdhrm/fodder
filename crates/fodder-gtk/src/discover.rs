//! HTML feed autodiscovery: when a subscribed URL turns out to be an HTML
//! page (e.g. a blog homepage), find the feed it advertises via
//! `<link rel="alternate" type="application/atom+xml" ...>` and subscribe to
//! that instead — the behavior users know from other readers.

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use fodder_core::fetch::{self, FetchOutcome};
use fodder_core::parse::{self, ParsedFeed};
use fodder_core::Error;

const FEED_TYPES: [&str; 4] = [
    "application/rss+xml",
    "application/atom+xml",
    "application/feed+json",
    "application/json",
];
const MAX_CANDIDATES: usize = 5;

/// Everything needed to store a validated subscription. `url` is the REAL
/// feed URL — the discovered one when the entered URL was an HTML page.
pub struct Subscription {
    pub parsed: ParsedFeed,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub url: String,
}

/// Fetch+parse `url`; if it is not a feed, try each feed URL the document
/// advertises. Returns the original parse error when nothing works out.
pub fn resolve_subscription(agent: &fetch::Agent, url: &str) -> fodder_core::Result<Subscription> {
    let fetched = match fetch::fetch_feed(agent, url, None, None)? {
        FetchOutcome::Fetched(fetched) => fetched,
        FetchOutcome::NotModified => return Err(Error::Http("empty 304 response".into())),
    };
    let parse_error = match parse::parse_feed(&fetched.body) {
        Ok(parsed) => {
            return Ok(Subscription {
                parsed,
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                url: fetched.permanent_url.unwrap_or_else(|| url.to_string()),
            })
        }
        Err(e) => e,
    };

    for candidate in discover_feed_urls(&fetched.body, url) {
        let Ok(FetchOutcome::Fetched(feed)) = fetch::fetch_feed(agent, &candidate, None, None)
        else {
            continue;
        };
        if let Ok(parsed) = parse::parse_feed(&feed.body) {
            return Ok(Subscription {
                parsed,
                etag: feed.etag,
                last_modified: feed.last_modified,
                url: feed.permanent_url.unwrap_or(candidate),
            });
        }
    }
    Err(parse_error)
}

/// Feed URLs advertised by an HTML document via `<link rel="alternate">`,
/// resolved against `base_url`, deduped, in document order.
pub fn discover_feed_urls(html: &[u8], base_url: &str) -> Vec<String> {
    let Ok(base) = ammonia::Url::parse(base_url) else {
        return Vec::new();
    };
    let dom = html5ever::parse_document(RcDom::default(), Default::default())
        .one(String::from_utf8_lossy(html).into_owned());
    let mut found = Vec::new();
    collect_links(&dom.document, &base, &mut found);
    found
}

fn collect_links(node: &Handle, base: &ammonia::Url, out: &mut Vec<String>) {
    if out.len() >= MAX_CANDIDATES {
        return;
    }
    if let NodeData::Element { name, attrs, .. } = &node.data {
        if name.local.as_ref() == "link" {
            let attrs = attrs.borrow();
            let attr = |wanted: &str| {
                attrs
                    .iter()
                    .find(|a| a.name.local.as_ref() == wanted)
                    .map(|a| a.value.to_string())
            };
            let rel = attr("rel").unwrap_or_default().to_ascii_lowercase();
            let mime = attr("type").unwrap_or_default().to_ascii_lowercase();
            if rel.split_ascii_whitespace().any(|r| r == "alternate")
                && FEED_TYPES.contains(&mime.trim())
            {
                if let Some(href) = attr("href") {
                    if let Ok(resolved) = base.join(href.trim()) {
                        let resolved = resolved.to_string();
                        if !out.contains(&resolved) {
                            out.push(resolved);
                        }
                    }
                }
            }
        }
    }
    for child in node.children.borrow().iter() {
        collect_links(child, base, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://tonsky.me/";

    // Verbatim from the live page that motivated this feature.
    const TONSKY_HEAD: &str = r#"<html><head>
      <link href="/atom.xml" rel="alternate" title="Nikita Prokopov’s blog" type="application/atom+xml">
      </head><body>hi</body></html>"#;

    #[test]
    fn discovers_relative_atom_link() {
        assert_eq!(
            discover_feed_urls(TONSKY_HEAD.as_bytes(), BASE),
            vec!["https://tonsky.me/atom.xml"]
        );
    }

    #[test]
    fn absolute_hrefs_and_document_order() {
        let html = r#"<head>
          <link rel="alternate" type="application/rss+xml" href="https://example.org/rss">
          <link rel="alternate" type="application/atom+xml" href="/atom">
        </head>"#;
        assert_eq!(
            discover_feed_urls(html.as_bytes(), "https://example.org/blog/post"),
            vec!["https://example.org/rss", "https://example.org/atom"]
        );
    }

    #[test]
    fn multi_value_rel_and_case_insensitivity() {
        let html = r#"<link rel="ALTERNATE home" TYPE="application/Atom+XML" href="feed.xml">"#;
        assert_eq!(
            discover_feed_urls(html.as_bytes(), "https://example.org/dir/"),
            vec!["https://example.org/dir/feed.xml"]
        );
    }

    #[test]
    fn ignores_non_feed_alternates_and_stylesheets() {
        let html = r#"<head>
          <link rel="alternate" type="text/html" href="/en">
          <link rel="stylesheet" type="text/css" href="/style.css">
          <link rel="alternate" hreflang="de" href="/de">
        </head>"#;
        assert!(discover_feed_urls(html.as_bytes(), BASE).is_empty());
    }

    #[test]
    fn dedups_and_caps_candidates() {
        let link = r#"<link rel="alternate" type="application/rss+xml" href="/rss">"#;
        let many: String = (0..10)
            .map(|i| format!(r#"<link rel="alternate" type="application/rss+xml" href="/rss{i}">"#))
            .collect();
        let html = format!("<head>{link}{link}{many}</head>");
        let found = discover_feed_urls(html.as_bytes(), BASE);
        assert_eq!(found.len(), MAX_CANDIDATES);
        assert_eq!(found[0], "https://tonsky.me/rss");
        // duplicate collapsed, then document order
        assert_eq!(found[1], "https://tonsky.me/rss0");
    }

    #[test]
    fn plain_text_or_empty_input_yields_nothing() {
        assert!(discover_feed_urls(b"just some text", BASE).is_empty());
        assert!(discover_feed_urls(b"", BASE).is_empty());
    }
}
