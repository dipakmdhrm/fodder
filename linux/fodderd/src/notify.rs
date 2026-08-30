//! Desktop notifications, batched one per feed and actionable (clicking opens
//! the newest article).

use fodder_core::APP_ID;
use notify_rust::{Hint, Notification};
use tokio::sync::mpsc::UnboundedSender;

use crate::state::OpenRequest;

/// A minimal view of a freshly-inserted article for the notification body.
pub struct NotifyItem {
    pub id: i64,
    pub title: String,
}

/// Show one notification summarizing the new articles for a feed.
///
/// The blocking `wait_for_action` (which lives until the user clicks or the
/// notification closes) runs on its own OS thread so it never ties up a tokio
/// worker. Clicking routes an [`OpenRequest::At`] pointing at the newest item.
pub fn notify_feed(
    feed_id: i64,
    feed_title: String,
    items: Vec<NotifyItem>,
    open_tx: UnboundedSender<OpenRequest>,
) {
    if items.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        let count = items.len();
        let summary = if count == 1 {
            feed_title.clone()
        } else {
            format!("{feed_title} — {count} new articles")
        };
        let body = items
            .iter()
            .take(4)
            .map(|it| format!("• {}", it.title))
            .collect::<Vec<_>>()
            .join("\n");

        // Newest item is first; the click action opens it.
        let newest = items.first().map(|it| it.id);

        let handle = Notification::new()
            .appname("Fodder Reader")
            .summary(&summary)
            .body(&body)
            .icon(APP_ID)
            .hint(Hint::DesktopEntry(APP_ID.to_string()))
            .action("default", "Open")
            .show();

        match handle {
            Ok(handle) => {
                handle.wait_for_action(|action| {
                    // "default" fires on a plain body click; some servers use
                    // the "__clicked" alias.
                    if action == "default" || action == "__clicked" {
                        let _ = open_tx.send(OpenRequest::At {
                            feed_id,
                            article_id: newest,
                        });
                    }
                });
            }
            Err(e) => tracing::warn!("notification for feed {feed_id} failed: {e}"),
        }
    });
}

/// Show the daily reading reminder. Clicking it opens the viewer.
pub fn notify_reminder(unread: i64, open_tx: UnboundedSender<OpenRequest>) {
    std::thread::spawn(move || {
        let body = if unread == 1 {
            "You have 1 unread article.".to_string()
        } else {
            format!("You have {unread} unread articles.")
        };
        let handle = Notification::new()
            .appname("Fodder Reader")
            .summary("Time to catch up")
            .body(&body)
            .icon(APP_ID)
            .hint(Hint::DesktopEntry(APP_ID.to_string()))
            .action("default", "Open")
            .show();

        match handle {
            Ok(handle) => handle.wait_for_action(|action| {
                if action == "default" || action == "__clicked" {
                    let _ = open_tx.send(OpenRequest::Show);
                }
            }),
            Err(e) => tracing::warn!("reminder notification failed: {e}"),
        }
    });
}
