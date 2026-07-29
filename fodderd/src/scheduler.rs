//! The poll scheduler: wakes when feeds are due (or on a refresh request),
//! polls them with bounded concurrency, stores outcomes, and fires
//! notifications for genuinely-new articles.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use fodder_core::db::{articles, feeds};
use fodder_core::ipc::IpcMessage;
use fodder_core::models::{Feed, NewArticle};
use fodder_core::poller::{backoff_next, PollOutcome};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::notify::{self, NotifyItem};
use crate::state::AppCtx;

/// Base unit for per-feed exponential backoff after errors.
const BACKOFF_BASE: Duration = Duration::from_secs(60);
/// Fallback sleep when no feeds exist yet, so we still react to new subscriptions.
const IDLE_SLEEP: Duration = Duration::from_secs(60);

/// Run the scheduler until the process exits. `refresh_rx` carries manual poll
/// requests (`Some(feed_id)` for one feed, `None` for "all due now").
pub async fn run(ctx: AppCtx, mut refresh_rx: UnboundedReceiver<Option<i64>>) {
    loop {
        // Poll everything currently due.
        if let Err(e) = poll_due(&ctx).await {
            tracing::warn!("poll cycle error: {e}");
        }

        // Sleep until the next feed is due, or until a refresh request arrives.
        let wait = match next_wait(&ctx).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("computing next wake failed: {e}");
                IDLE_SLEEP
            }
        };
        tracing::debug!("scheduler sleeping for {:?}", wait);

        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            maybe = refresh_rx.recv() => {
                match maybe {
                    Some(Some(feed_id)) => {
                        if let Err(e) = poll_one(&ctx, feed_id).await {
                            tracing::warn!("manual poll of feed {feed_id} failed: {e}");
                        }
                    }
                    Some(None) => {
                        // A user-invoked "refresh now" forces every feed to be
                        // polled regardless of its schedule (conditional GET
                        // keeps this cheap — unchanged feeds return 304).
                        if let Err(e) = poll_all_now(&ctx).await {
                            tracing::warn!("manual refresh failed: {e}");
                        }
                    }
                    None => return, // all senders dropped; shutting down
                }
            }
        }
    }
}

/// Duration until the soonest `next_poll_at`, clamped to a sane range.
async fn next_wait(ctx: &AppCtx) -> anyhow::Result<Duration> {
    let feeds = ctx.with_conn(|c| feeds::list_feeds(c)).await?;
    let now = Utc::now();
    let soonest = feeds.iter().map(|f| f.next_poll_at).min();
    Ok(match soonest {
        Some(t) if t > now => (t - now)
            .to_std()
            .unwrap_or(IDLE_SLEEP)
            .min(Duration::from_secs(3600)),
        Some(_) => Duration::from_millis(0), // something is already overdue
        None => IDLE_SLEEP,
    })
}

/// Poll every feed whose schedule is due now.
async fn poll_due(ctx: &AppCtx) -> anyhow::Result<()> {
    let now = Utc::now();
    let due = ctx.with_conn(move |c| feeds::feeds_due(c, now)).await?;
    if due.is_empty() {
        return Ok(());
    }
    tracing::info!("polling {} due feed(s)", due.len());
    poll_feeds(ctx, due).await
}

/// Force-poll every feed now, regardless of schedule (user-invoked refresh).
async fn poll_all_now(ctx: &AppCtx) -> anyhow::Result<()> {
    let feeds = ctx.with_conn(|c| feeds::list_feeds(c)).await?;
    if feeds.is_empty() {
        tracing::info!("refresh: no feeds subscribed");
        return Ok(());
    }
    tracing::info!("refresh: force-polling all {} feed(s)", feeds.len());
    poll_feeds(ctx, feeds).await
}

/// Poll a single feed by id (manual refresh), regardless of schedule.
async fn poll_one(ctx: &AppCtx, feed_id: i64) -> anyhow::Result<()> {
    let feed = ctx.with_conn(move |c| feeds::get_feed(c, feed_id)).await?;
    match feed {
        Some(feed) => poll_feeds(ctx, vec![feed]).await,
        None => Ok(()),
    }
}

/// Poll the given feeds concurrently and store each outcome.
async fn poll_feeds(ctx: &AppCtx, feeds_to_poll: Vec<Feed>) -> anyhow::Result<()> {
    let by_id: HashMap<i64, Feed> =
        feeds_to_poll.iter().map(|f| (f.id, f.clone())).collect();

    let outcomes = ctx.poller.poll_all(feeds_to_poll).await;
    let interval = ctx.config.poll_interval();
    let now = Utc::now();
    let mut any_new = false;

    for (feed_id, outcome) in outcomes {
        let Some(feed) = by_id.get(&feed_id) else {
            continue;
        };
        match outcome {
            PollOutcome::Updated {
                title,
                etag,
                last_modified,
                items,
            } => {
                let new_count =
                    store_updated(ctx, feed, title, etag, last_modified, items, now + interval)
                        .await?;
                any_new |= new_count > 0;
            }
            PollOutcome::NotModified => {
                let next = now + interval;
                ctx.with_conn(move |c| feeds::reschedule(c, feed_id, next))
                    .await?;
            }
            PollOutcome::RateLimited { retry_after } => {
                let next = now
                    + chrono::Duration::from_std(retry_after)
                        .unwrap_or_else(|_| chrono::Duration::minutes(5));
                tracing::info!("feed {feed_id} rate-limited; retrying at {next}");
                ctx.with_conn(move |c| feeds::reschedule(c, feed_id, next))
                    .await?;
            }
            PollOutcome::Error(err) => {
                let error_count = feed.error_count + 1;
                let backoff = backoff_next(error_count, BACKOFF_BASE);
                let next = now
                    + chrono::Duration::from_std(backoff)
                        .unwrap_or_else(|_| chrono::Duration::hours(6));
                tracing::warn!("feed {feed_id} error (#{error_count}): {err}");
                ctx.with_conn(move |c| {
                    feeds::update_feed_error(c, feed_id, &err, error_count, next)
                })
                .await?;
            }
        }
    }

    if any_new {
        ctx.send_to_viewer(IpcMessage::FeedsChanged);
    }
    Ok(())
}

/// Store an updated feed: update the title, insert new articles (deduped),
/// record success + validators, and notify about the new items. Returns the
/// number of new articles.
async fn store_updated(
    ctx: &AppCtx,
    feed: &Feed,
    title: Option<String>,
    etag: Option<String>,
    last_modified: Option<String>,
    items: Vec<NewArticle>,
    next_poll_at: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let feed_id = feed.id;
    let old_title = feed.title.clone();

    // Do all the writes in one blocking hop and return the new articles' details.
    let new_items: Vec<NotifyItem> = ctx
        .with_conn(move |c| {
            // Update the title if the feed now advertises a (different) one.
            if let Some(t) = &title {
                if !t.is_empty() && *t != old_title {
                    feeds::update_feed_title(c, feed_id, t)?;
                }
            }

            let new_ids = articles::insert_new_articles(c, feed_id, &items)?;

            feeds::update_feed_success(
                c,
                feed_id,
                etag.as_deref(),
                last_modified.as_deref(),
                next_poll_at,
            )?;

            // Fetch the freshly-inserted rows (newest first) for the notification.
            let mut out = Vec::new();
            for id in &new_ids {
                if let Some(a) = articles::get_article(c, *id)? {
                    out.push(NotifyItem {
                        id: a.id,
                        title: a.title,
                    });
                }
            }
            out.sort_by(|a, b| b.id.cmp(&a.id)); // newest (highest id) first
            Ok(out)
        })
        .await?;

    let count = new_items.len();
    if count > 0 {
        tracing::info!("feed {feed_id}: {count} new article(s)");
        // Use the possibly-updated title for the notification.
        let feed_title = ctx
            .with_conn(move |c| feeds::get_feed(c, feed_id))
            .await?
            .map(|f| f.title)
            .unwrap_or_default();
        notify::notify_feed(feed_id, feed_title, new_items, ctx.open_tx.clone());
    }
    Ok(count)
}
