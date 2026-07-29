//! Manual end-to-end check of the M1 core logic: feed discovery + a real
//! conditional GET + feed-rs parsing, printed to stdout.
//!
//! Usage:
//!     cargo run -p fodder-core --example poll -- <url>
//!
//! Examples:
//!     cargo run -p fodder-core --example poll -- https://blog.rust-lang.org/
//!     cargo run -p fodder-core --example poll -- https://blog.rust-lang.org/feed.xml
//!
//! Pass a site URL to see feed discovery pick out its `<link rel=alternate>`
//! feeds, or pass a feed URL directly to see it polled and parsed.

use fodder_core::discovery::{self, DiscoveryResult};
use fodder_core::models::Feed;
use fodder_core::poller::{PollOutcome, Poller};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let url = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run -p fodder-core --example poll -- <url>");
        std::process::exit(2);
    });

    let poller = Poller::new(4);

    println!("== Discovery for {url} ==");
    match discovery::resolve_feed(poller.client(), &url).await? {
        DiscoveryResult::DirectFeed { url, title } => {
            println!("  direct feed: {title}\n  ({url})");
            poll_and_print(&poller, &url).await;
        }
        DiscoveryResult::Candidates(feeds) => {
            println!("  {} feed link(s) found on the page:", feeds.len());
            for (i, f) in feeds.iter().enumerate() {
                println!(
                    "   [{i}] {:?}  {}  ({})",
                    f.kind,
                    f.title.as_deref().unwrap_or("(untitled)"),
                    f.url
                );
            }
            if let Some(first) = feeds.first() {
                println!("\n== Polling first candidate ==");
                poll_and_print(&poller, &first.url).await;
            }
        }
        DiscoveryResult::None => println!("  no feed found at this URL"),
    }

    Ok(())
}

/// Poll one feed URL (fresh, so no validators are sent) and summarize the parse.
async fn poll_and_print(poller: &Poller, feed_url: &str) {
    // A throwaway Feed with no stored validators, so this is an unconditional GET.
    let feed = Feed {
        id: 0,
        url: feed_url.to_string(),
        title: String::new(),
        etag: None,
        last_modified: None,
        last_error: None,
        error_count: 0,
        next_poll_at: chrono::Utc::now(),
        created_at: chrono::Utc::now(),
    };

    match poller.poll_feed(&feed).await {
        PollOutcome::Updated {
            title,
            etag,
            last_modified,
            items,
        } => {
            println!("  title: {}", title.as_deref().unwrap_or("(none)"));
            println!("  etag: {etag:?}  last_modified: {last_modified:?}");
            println!("  {} item(s):", items.len());
            for item in items.iter().take(5) {
                println!(
                    "    - {}\n      guid={}  url={}",
                    item.title,
                    item.guid,
                    item.url.as_deref().unwrap_or("-")
                );
            }
        }
        PollOutcome::NotModified => println!("  304 Not Modified"),
        PollOutcome::RateLimited { retry_after } => {
            println!("  rate limited, retry after {retry_after:?}")
        }
        PollOutcome::Error(e) => println!("  error: {e}"),
    }
}
