//! Subscription resolution: given a URL, decide whether it is itself a feed or
//! an HTML page that links to one (or several) feeds.

use reqwest::{Client, Url};
use scraper::{Html, Selector};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
}

/// What kind of feed a discovered link points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Rss,
    Atom,
    Json,
}

impl FeedKind {
    fn from_mime(mime: &str) -> Option<Self> {
        // Compare on the essence, ignoring parameters/whitespace/case.
        let m = mime
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match m.as_str() {
            "application/rss+xml" => Some(FeedKind::Rss),
            "application/atom+xml" => Some(FeedKind::Atom),
            "application/json" | "application/feed+json" => Some(FeedKind::Json),
            _ => None,
        }
    }
}

/// A feed link found on an HTML page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFeed {
    pub url: String,
    pub title: Option<String>,
    pub kind: FeedKind,
}

/// The result of resolving a subscription URL.
#[derive(Debug)]
pub enum DiscoveryResult {
    /// The URL is itself a valid feed.
    DirectFeed { url: String, title: String },
    /// The URL is an HTML page linking to one or more feeds.
    Candidates(Vec<DiscoveredFeed>),
    /// Neither a feed nor a page with feed links.
    None,
}

/// Resolve `input_url`: fetch it, try to parse it as a feed directly, and
/// otherwise scan the HTML for `<link rel="alternate">` feed references.
pub async fn resolve_feed(
    client: &Client,
    input_url: &str,
) -> Result<DiscoveryResult, DiscoveryError> {
    let resp = client.get(input_url).send().await?.error_for_status()?;
    let final_url = resp.url().clone();
    let bytes = resp.bytes().await?;

    // 1. Is the fetched document itself a feed?
    if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
        let title = feed
            .title
            .map(|t| t.content)
            .unwrap_or_else(|| final_url.to_string());
        return Ok(DiscoveryResult::DirectFeed {
            url: final_url.to_string(),
            title,
        });
    }

    // 2. Otherwise treat it as HTML and look for alternate feed links.
    let html = String::from_utf8_lossy(&bytes);
    let candidates = extract_feed_links(&html, &final_url);
    if candidates.is_empty() {
        Ok(DiscoveryResult::None)
    } else {
        Ok(DiscoveryResult::Candidates(candidates))
    }
}

/// Fetch a resolved feed URL and return its title, for the confirm-preview step.
pub async fn preview_title(client: &Client, feed_url: &str) -> Result<String, DiscoveryError> {
    let resp = client.get(feed_url).send().await?.error_for_status()?;
    let final_url = resp.url().clone();
    let bytes = resp.bytes().await?;
    let feed = feed_rs::parser::parse(&bytes[..])
        .map_err(|_| DiscoveryError::InvalidUrl(feed_url.to_string()))?;
    Ok(feed
        .title
        .map(|t| t.content)
        .unwrap_or_else(|| final_url.to_string()))
}

/// Pure helper: extract `<link rel="alternate">` feed links from HTML, resolving
/// relative hrefs against `base`. Testable without any network.
pub fn extract_feed_links(html: &str, base: &Url) -> Vec<DiscoveredFeed> {
    let doc = Html::parse_document(html);
    // `rel~=alternate` matches rel lists containing the word "alternate".
    let selector = match Selector::parse("link[rel~=alternate][type]") {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for el in doc.select(&selector) {
        let Some(ty) = el.value().attr("type") else {
            continue;
        };
        let Some(kind) = FeedKind::from_mime(ty) else {
            continue;
        };
        let Some(href) = el.value().attr("href") else {
            continue;
        };
        let Ok(resolved) = base.join(href) else {
            continue;
        };
        let title = el.value().attr("title").map(str::to_string);
        out.push(DiscoveredFeed {
            url: resolved.to_string(),
            title,
            kind,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://blog.example.com/index.html").unwrap()
    }

    #[test]
    fn discovers_multiple_alternate_links_with_relative_hrefs() {
        let html = r#"<html><head>
            <link rel="alternate" type="application/rss+xml" title="RSS" href="/feed.xml">
            <link rel="alternate" type="application/atom+xml" title="Atom" href="atom.xml">
            <link rel="alternate" type="application/feed+json" href="https://cdn.example.com/f.json">
        </head></html>"#;
        let feeds = extract_feed_links(html, &base());
        assert_eq!(feeds.len(), 3);
        // Relative hrefs resolved against the base URL.
        assert_eq!(feeds[0].url, "https://blog.example.com/feed.xml");
        assert_eq!(feeds[0].kind, FeedKind::Rss);
        assert_eq!(feeds[1].url, "https://blog.example.com/atom.xml");
        assert_eq!(feeds[1].kind, FeedKind::Atom);
        // Absolute href preserved.
        assert_eq!(feeds[2].url, "https://cdn.example.com/f.json");
        assert_eq!(feeds[2].kind, FeedKind::Json);
    }

    #[test]
    fn json_feed_alternate_recognized() {
        let html = r#"<link rel="alternate" type="application/json" href="/feed.json">"#;
        let feeds = extract_feed_links(html, &base());
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].kind, FeedKind::Json);
    }

    #[test]
    fn ignores_non_feed_and_non_alternate_links() {
        let html = r#"<html><head>
            <link rel="stylesheet" type="text/css" href="/style.css">
            <link rel="alternate" type="text/html" href="/amp">
            <link rel="icon" href="/favicon.ico">
        </head></html>"#;
        assert!(extract_feed_links(html, &base()).is_empty());
    }

    #[test]
    fn mime_with_charset_parameter_still_matches() {
        let html = r#"<link rel="alternate" type="application/rss+xml; charset=utf-8" href="/f">"#;
        let feeds = extract_feed_links(html, &base());
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].kind, FeedKind::Rss);
    }
}
