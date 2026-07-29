//! Desktop notifications, batched one per feed and actionable (clicking opens
//! the newest article).

use notify_rust::Notification;
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
            .icon("application-rss+xml")
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
