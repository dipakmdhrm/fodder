//! Fetch+parse fan-out over a few worker threads; all DB writes stay on the
//! calling thread so transactions never span network I/O.

use std::collections::VecDeque;
use std::sync::Mutex;

use fodder_core::fetch::{self, FetchOutcome};
use fodder_core::models::Feed;
use fodder_core::parse::{self, ParsedFeed};
use fodder_core::Db;

const MAX_WORKERS: usize = 4;

pub struct PollStats {
    pub new_items: u32,
    pub feeds_with_new: u32,
}

enum WorkOutcome {
    NotModified,
    Content {
        parsed: ParsedFeed,
        etag: Option<String>,
        last_modified: Option<String>,
        permanent_url: Option<String>,
    },
    Failed(String),
}

pub fn poll_feeds(db: &mut Db, feeds: &[Feed], now: i64) -> PollStats {
    let queue: Mutex<VecDeque<&Feed>> = Mutex::new(feeds.iter().collect());
    let results: Mutex<Vec<(i64, WorkOutcome)>> = Mutex::new(Vec::with_capacity(feeds.len()));

    std::thread::scope(|scope| {
        for _ in 0..MAX_WORKERS.min(feeds.len()) {
            scope.spawn(|| {
                let agent = fetch::agent();
                loop {
                    let feed = match queue.lock().unwrap().pop_front() {
                        Some(feed) => feed,
                        None => return,
                    };
                    let outcome = fetch_one(&agent, feed);
                    results.lock().unwrap().push((feed.id, outcome));
                }
            });
        }
    });

    let mut stats = PollStats {
        new_items: 0,
        feeds_with_new: 0,
    };
    for (feed_id, outcome) in results.into_inner().unwrap() {
        if let Err(e) = apply_outcome(db, feed_id, outcome, now, &mut stats) {
            log::warn!("feed {feed_id}: failed to store poll result: {e}");
        }
    }
    stats
}

fn fetch_one(agent: &fetch::Agent, feed: &Feed) -> WorkOutcome {
    match fetch::fetch_feed(
        agent,
        &feed.url,
        feed.etag.as_deref(),
        feed.last_modified.as_deref(),
    ) {
        Ok(FetchOutcome::NotModified) => WorkOutcome::NotModified,
        Ok(FetchOutcome::Fetched(fetched)) => match parse::parse_feed(&fetched.body) {
            Ok(parsed) => WorkOutcome::Content {
                parsed,
                etag: fetched.etag,
                last_modified: fetched.last_modified,
                permanent_url: fetched.permanent_url,
            },
            Err(e) => WorkOutcome::Failed(e.to_string()),
        },
        Err(e) => WorkOutcome::Failed(e.to_string()),
    }
}

fn apply_outcome(
    db: &mut Db,
    feed_id: i64,
    outcome: WorkOutcome,
    now: i64,
    stats: &mut PollStats,
) -> fodder_core::Result<()> {
    match outcome {
        WorkOutcome::NotModified => {
            // Keep the stored ETag/Last-Modified as-is.
            let feed = db.get_feed(feed_id)?;
            let (etag, lm) = feed
                .map(|f| (f.etag, f.last_modified))
                .unwrap_or((None, None));
            db.record_poll(feed_id, etag.as_deref(), lm.as_deref(), "ok", now)?;
        }
        WorkOutcome::Content {
            parsed,
            etag,
            last_modified,
            permanent_url,
        } => {
            let new = db.insert_items(feed_id, &parsed.items, now)?;
            db.update_feed_metadata(
                feed_id,
                &parsed.title,
                parsed.site_url.as_deref(),
                parsed.description.as_deref(),
            )?;
            if let Some(url) = permanent_url {
                log::info!("feed {feed_id} moved permanently to {url}");
                db.update_feed_url(feed_id, &url)?;
            }
            db.record_poll(
                feed_id,
                etag.as_deref(),
                last_modified.as_deref(),
                "ok",
                now,
            )?;
            if new > 0 {
                stats.new_items += new;
                stats.feeds_with_new += 1;
            }
        }
        WorkOutcome::Failed(message) => {
            log::warn!("feed {feed_id}: poll failed: {message}");
            db.record_poll(feed_id, None, None, &message, now)?;
        }
    }
    Ok(())
}
