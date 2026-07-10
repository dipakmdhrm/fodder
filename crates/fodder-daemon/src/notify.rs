use crate::util;

/// One summarized notification per poll round. Clicking it opens the viewer,
/// but not every notification server delivers actions, so the body must stand
/// on its own.
pub fn new_items(new_count: u32, feed_count: u32) {
    let body = if feed_count == 1 {
        format!("{new_count} new article(s)")
    } else {
        format!("{new_count} new article(s) in {feed_count} feeds")
    };
    let shown = notify_rust::Notification::new()
        .summary("Fodder")
        .body(&body)
        .icon("application-rss+xml-symbolic")
        .appname("Fodder")
        .action("default", "Open")
        .show();
    match shown {
        Ok(handle) => {
            // wait_for_action blocks until the notification is acted on or
            // closed; park it on a short-lived thread.
            std::thread::spawn(move || {
                handle.wait_for_action(|action| {
                    if action == "default" {
                        util::spawn_viewer();
                    }
                });
            });
        }
        Err(e) => log::warn!("could not show notification: {e}"),
    }
}
