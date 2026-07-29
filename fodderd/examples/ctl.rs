//! Tiny control client for a running `fodderd`, for manual testing of the IPC
//! surface before the GTK viewer exists.
//!
//! Usage:
//!     cargo run -p fodderd --example ctl -- ping
//!     cargo run -p fodderd --example ctl -- subscribe <feed-url> <title>
//!     cargo run -p fodderd --example ctl -- refresh [feed_id]
//!     cargo run -p fodderd --example ctl -- open
//!     cargo run -p fodderd --example ctl -- list          # read the DB directly
//!     cargo run -p fodderd --example ctl -- rm <feed_id>  # unsubscribe + cascade

use std::time::Duration;

use fodder_core::db::{articles, feeds, Db};
use fodder_core::ipc::{self, IpcMessage};
use fodder_core::paths;
use tokio::net::UnixStream;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let cmd = args.next().unwrap_or_default();

    // These commands act locally and don't need the daemon.
    match cmd.as_str() {
        "list" => return list_feeds(),
        "rm" => {
            let id: i64 = args.next().expect("rm needs <feed_id>").parse()?;
            return remove_feed(id);
        }
        "autostart" => {
            return autostart(args.next().as_deref());
        }
        _ => {}
    }

    let msg = match cmd.as_str() {
        "ping" => IpcMessage::Ping,
        "open" => IpcMessage::OpenViewer,
        "refresh" => IpcMessage::RefreshNow {
            feed_id: args.next().and_then(|s| s.parse().ok()),
        },
        "subscribe" => {
            let feed_url = args.next().expect("subscribe needs <feed-url>");
            let title = args.next().unwrap_or_else(|| feed_url.clone());
            IpcMessage::SubscribeResolved { feed_url, title }
        }
        other => {
            eprintln!("unknown command: {other:?}");
            eprintln!("commands: ping | subscribe <url> <title> | refresh [id] | open");
            std::process::exit(2);
        }
    };

    let socket = paths::daemon_socket_path()?;
    let mut stream = UnixStream::connect(&socket).await?;
    ipc::write_msg(&mut stream, &msg).await?;
    println!("-> {msg:?}");

    match tokio::time::timeout(Duration::from_secs(5), ipc::read_msg(&mut stream)).await {
        Ok(Ok(Some(reply))) => println!("<- {reply:?}"),
        Ok(Ok(None)) => println!("<- (connection closed)"),
        Ok(Err(e)) => println!("<- read error: {e}"),
        Err(_) => println!("<- (no reply within 5s)"),
    }
    Ok(())
}

/// Print every subscribed feed with its unread count and error state.
fn list_feeds() -> anyhow::Result<()> {
    let db = Db::open(&paths::db_path()?)?;
    let feeds = feeds::list_feeds(db.conn())?;
    let unread = articles::unread_counts(db.conn())?;
    if feeds.is_empty() {
        println!("(no feeds subscribed)");
        return Ok(());
    }
    for f in feeds {
        let n = unread.get(&f.id).copied().unwrap_or(0);
        let status = match &f.last_error {
            Some(e) => format!("ERROR(#{}): {e}", f.error_count),
            None => "ok".to_string(),
        };
        println!(
            "[{}] {}  ({} unread)\n     url: {}\n     status: {}",
            f.id, f.title, n, f.url, status
        );
    }
    Ok(())
}

/// Unsubscribe a feed by id; its articles cascade-delete.
fn remove_feed(id: i64) -> anyhow::Result<()> {
    let db = Db::open(&paths::db_path()?)?;
    feeds::delete_feed(db.conn(), id)?;
    println!("removed feed {id} (and its articles)");
    Ok(())
}

/// Query or toggle the autostart `.desktop` entry.
fn autostart(action: Option<&str>) -> anyhow::Result<()> {
    use fodder_core::autostart;
    match action {
        Some("on") => {
            autostart::enable()?;
            println!("autostart enabled -> {}", autostart::desktop_path()?.display());
        }
        Some("off") => {
            autostart::disable()?;
            println!("autostart disabled");
        }
        Some("status") | None => {
            println!(
                "autostart: {} ({})",
                if autostart::is_enabled() { "on" } else { "off" },
                autostart::desktop_path()?.display()
            );
        }
        Some(other) => {
            eprintln!("usage: autostart [status|on|off], got {other:?}");
            std::process::exit(2);
        }
    }
    Ok(())
}
