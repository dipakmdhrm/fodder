//! fodder-daemon: headless feed poller.
//!
//! Single-threaded scheduler around a Condvar; D-Bus method handlers and the
//! tray run on their own service threads and only flip wake flags.

mod dbus;
mod notify;
mod poll;
mod tray;
mod util;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use fodder_core::{now_unix, schedule, Db};

/// Fallback sleep when there is nothing to schedule (no feeds yet).
const IDLE_SLEEP: Duration = Duration::from_secs(3600);

#[derive(Default)]
pub struct Flags {
    pub quit: bool,
    pub poll_now: bool,
    pub settings_changed: bool,
}

#[derive(Default)]
pub struct Shared {
    state: Mutex<Flags>,
    cond: Condvar,
    pub unread: AtomicU32,
}

impl Shared {
    pub fn signal(&self, set: impl FnOnce(&mut Flags)) {
        let mut flags = self.state.lock().unwrap();
        set(&mut flags);
        self.cond.notify_all();
    }
}

fn main() -> std::process::ExitCode {
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("fodder-daemon {}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            log::error!("fatal: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> fodder_core::Result<()> {
    let mut db = Db::open_default()?;
    let shared = Arc::new(Shared::default());
    shared.unread.store(db.unread_total()?, Ordering::SeqCst);

    let dbus_conn = match dbus::serve(shared.clone()) {
        Ok(conn) => Some(conn),
        Err(dbus::ServeError::NameTaken) => {
            log::info!("another fodder-daemon owns the bus name; exiting");
            return Ok(());
        }
        Err(dbus::ServeError::Other(e)) => {
            // No session bus (or broken one): keep polling, skip IPC/tray.
            log::warn!("D-Bus unavailable, running degraded: {e}");
            None
        }
    };

    let mut settings = db.settings()?;
    let mut tray = tray::TrayState::new(shared.clone());
    if dbus_conn.is_some() {
        tray.set_visible(
            settings.run_in_background,
            shared.unread.load(Ordering::SeqCst),
        );
    }

    let mut force_poll = false;
    loop {
        let now = now_unix();
        let feeds = db.list_feeds()?;
        let due: Vec<_> = feeds
            .iter()
            .filter(|f| {
                force_poll
                    || schedule::is_due(f.last_polled_at, settings.poll_interval_minutes, now)
            })
            .cloned()
            .collect();
        force_poll = false;

        if !due.is_empty() {
            log::info!("polling {} feed(s)", due.len());
            let stats = poll::poll_feeds(&mut db, &due, now);
            let unread = db.unread_total()?;
            shared.unread.store(unread, Ordering::SeqCst);
            tray.set_unread(unread);
            if stats.new_items > 0 {
                log::info!(
                    "{} new item(s) in {} feed(s)",
                    stats.new_items,
                    stats.feeds_with_new
                );
                if let Some(conn) = &dbus_conn {
                    dbus::emit_items_added(conn, stats.new_items, unread);
                }
                if settings.notifications {
                    notify::new_items(stats.new_items, stats.feeds_with_new);
                }
            }
        }

        // Sleep until the earliest feed is due or a flag wakes us.
        let feeds = db.list_feeds()?;
        let sleep = schedule::seconds_until_next_due(
            feeds.iter().map(|f| f.last_polled_at),
            settings.poll_interval_minutes,
            now_unix(),
        )
        .map(|secs| Duration::from_secs(secs.max(1)))
        .unwrap_or(IDLE_SLEEP);

        let deadline = Instant::now() + sleep;
        let mut flags = shared.state.lock().unwrap();
        loop {
            if flags.quit {
                return Ok(());
            }
            if flags.poll_now {
                flags.poll_now = false;
                force_poll = true;
                break;
            }
            if flags.settings_changed {
                flags.settings_changed = false;
                drop(flags);
                settings = db.settings()?;
                let unread = shared.unread.load(Ordering::SeqCst);
                if dbus_conn.is_some() {
                    tray.set_visible(settings.run_in_background, unread);
                }
                flags = shared.state.lock().unwrap();
                // Re-enter the outer loop so the new interval reschedules.
                break;
            }
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() {
                break;
            }
            let (guard, _) = shared.cond.wait_timeout(flags, timeout).unwrap();
            flags = guard;
        }
    }
}
