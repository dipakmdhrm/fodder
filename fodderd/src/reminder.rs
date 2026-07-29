//! The daily reading-reminder task.
//!
//! Once a day, at the user's chosen local time, send a reminder if there are
//! unread articles and the viewer isn't already open. Recomputes whenever the
//! config changes (via `ctx.reminder_reload`).

use std::time::Duration;

use chrono::{Local, TimeZone};
use fodder_core::db::articles;

use crate::notify;
use crate::state::AppCtx;

/// Run the reminder loop until the process exits.
pub async fn run(ctx: AppCtx) {
    loop {
        let cfg = ctx.config();
        let hm = if cfg.notifications_enabled && cfg.daily_reminder_enabled {
            cfg.reminder_hm()
        } else {
            None
        };

        let Some((hour, minute)) = hm else {
            // Disabled or invalid — idle until the config changes.
            ctx.reminder_reload.notified().await;
            continue;
        };

        let wait = duration_until_next(hour, minute);
        tracing::debug!("daily reminder scheduled in {:?}", wait);

        tokio::select! {
            _ = tokio::time::sleep(wait) => {
                fire(&ctx).await;
                // Avoid re-firing within the same minute before we recompute.
                tokio::time::sleep(Duration::from_secs(61)).await;
            }
            _ = ctx.reminder_reload.notified() => {
                // Config changed; loop and recompute the next fire time.
            }
        }
    }
}

/// Decide whether to send the reminder now, and do so.
async fn fire(ctx: &AppCtx) {
    let cfg = ctx.config();
    if !(cfg.notifications_enabled && cfg.daily_reminder_enabled) {
        return;
    }
    if ctx.viewer_alive() {
        tracing::info!("daily reminder skipped: viewer is open");
        return;
    }
    let unread = match ctx.with_conn(|c| articles::total_unread(c)).await {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!("reminder unread query failed: {e}");
            return;
        }
    };
    if unread <= 0 {
        tracing::info!("daily reminder skipped: nothing unread");
        return;
    }
    tracing::info!("daily reminder: {unread} unread article(s)");
    notify::notify_reminder(unread, ctx.open_tx.clone());
}

/// Duration from now until the next local occurrence of `hour:minute`.
fn duration_until_next(hour: u32, minute: u32) -> Duration {
    let now = Local::now();
    let target = local_at(now.date_naive(), hour, minute).filter(|t| *t > now);
    let target = target.unwrap_or_else(|| {
        let tomorrow = now.date_naive() + chrono::Duration::days(1);
        local_at(tomorrow, hour, minute).unwrap_or(now + chrono::Duration::minutes(1))
    });
    (target - now).to_std().unwrap_or(Duration::from_secs(60))
}

/// A `Local` datetime for `date` at `hour:minute`, resolving DST ambiguity by
/// taking the earliest valid instant.
fn local_at(date: chrono::NaiveDate, hour: u32, minute: u32) -> Option<chrono::DateTime<Local>> {
    let naive = date.and_hms_opt(hour, minute, 0)?;
    Local.from_local_datetime(&naive).earliest()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_occurrence_is_within_a_day() {
        // Whatever the target, the next occurrence is always in (0, 24h].
        for &(h, m) in &[(0, 0), (10, 0), (23, 59), (12, 30)] {
            let d = duration_until_next(h, m);
            assert!(d <= Duration::from_secs(24 * 3600 + 60), "{h}:{m} -> {d:?}");
        }
    }
}
