//! StatusNotifierItem system-tray icon via `ksni`, with a supervisor that
//! re-registers it when the SNI host restarts.
//!
//! Registration is best-effort: on desktops without an SNI host (e.g. vanilla
//! GNOME Shell without an AppIndicator extension) the initial spawn fails and we
//! degrade gracefully — the daemon keeps polling and notifying, and the viewer
//! can still be opened via a notification click or by re-running `fodder`.
//!
//! **Why the supervisor.** `ksni` already re-registers our item when the
//! watcher's D-Bus *name* comes and goes on a still-live connection (a plain
//! GNOME-on-Xorg shell restart). It does NOT recover when its own D-Bus
//! *connection* dies: the failure we hit when a Wayland session crashed out from
//! under a resident daemon. `ksni`'s internal `NameOwnerChanged` stream ended,
//! its service task went idle, and the icon vanished until a manual restart.
//! [`run_supervised`] closes that gap by polling the watcher's owner on an
//! *independent* D-Bus connection and re-`spawn`-ing the tray whenever a fresh
//! host appears (session relogin, shell restart, or a tray extension enabled
//! after we started).
//!
//! Boundary: a host that keeps the *same* bus owner while `ksni`'s connection
//! dies underneath it (rare — the owner almost always changes on the events that
//! kill a connection) is not detected, because we key rebuilds off owner
//! identity. The common session-restart case, which is what stranded the icon,
//! is covered.

use std::sync::Arc;
use std::time::Duration;

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Tray, TrayMethods};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::state::OpenRequest;

use fodder_core::APP_ID;

/// The SNI host's well-known bus name.
const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";

/// How often the supervisor re-checks the SNI host. The icon reappearing within
/// this window of a session restart is imperceptible in practice, and the probe
/// (one cached D-Bus method call) is cheap.
const POLL_INTERVAL: Duration = Duration::from_secs(15);

/// The tray model. Holds only cloneable senders so menu callbacks can hand work
/// off to the daemon without blocking the menu.
pub struct FodderTray {
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
}

impl Tray for FodderTray {
    fn id(&self) -> String {
        APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Fodder Reader".to_string()
    }

    fn icon_name(&self) -> String {
        // Resolves once the icon set is installed (./install.sh). Before any
        // install it falls back to the host's default.
        APP_ID.to_string()
    }

    /// Left-click opens the viewer.
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.open_tx.send(OpenRequest::Show);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open".into(),
                icon_name: "document-open".into(),
                activate: Box::new(|t: &mut FodderTray| {
                    let _ = t.open_tx.send(OpenRequest::Show);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Refresh all feeds".into(),
                icon_name: "view-refresh".into(),
                activate: Box::new(|t: &mut FodderTray| {
                    let _ = t.refresh_tx.send(None);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut FodderTray| {
                    t.shutdown.notify_one();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Try to register the tray once. Returns the handle on success (keep it alive
/// for the tray's lifetime), or `None` if no SNI host is currently available.
async fn try_spawn(
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
) -> Option<Handle<FodderTray>> {
    let tray = FodderTray {
        open_tx,
        refresh_tx,
        shutdown,
    };
    match tray.spawn().await {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::warn!(
                "no system tray available ({e}); continuing without it \
                 (notifications and `fodder` re-run still open the viewer)"
            );
            None
        }
    }
}

/// Decide whether to (re)build the tray this tick.
///
/// `prev_owner`/`cur_owner` are the [`WATCHER_BUS_NAME`] unique-name owner as
/// seen last tick and now (`None` = no host present). `have_live_tray` is
/// whether we currently hold a registered item.
///
/// Rebuild when a host is present AND either we have no live item, or the host's
/// identity changed since last tick (a restart/relogin brought a fresh host that
/// never saw our item). No host present => nothing to register with, so leave
/// any existing item in place for `ksni` to re-offer if the same host returns.
pub(crate) fn tray_needs_rebuild(
    prev_owner: Option<&str>,
    cur_owner: Option<&str>,
    have_live_tray: bool,
) -> bool {
    match cur_owner {
        None => false,
        Some(_) if !have_live_tray => true,
        Some(cur) => prev_owner != Some(cur),
    }
}

/// Read the current owner (unique bus name) of the SNI host, or `None` if no
/// host is registered. Errors other than "no owner" mean our probe connection
/// is unhealthy and should be rebuilt.
async fn watcher_owner(conn: &zbus::Connection) -> zbus::Result<Option<String>> {
    let dbus = zbus::fdo::DBusProxy::new(conn).await?;
    let name = zbus::names::BusName::try_from(WATCHER_BUS_NAME)
        .expect("watcher bus name is a valid D-Bus name");
    match dbus.get_name_owner(name).await {
        Ok(owner) => Ok(Some(owner.to_string())),
        Err(zbus::fdo::Error::NameHasNoOwner(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Run the tray for the daemon's lifetime: register once, then supervise.
///
/// `shutdown` is the daemon's global signal, handed to [`FodderTray`] so the
/// menu's Quit item can request a full daemon shutdown. `tray_shutdown` is a
/// dedicated signal owned solely by this task: the daemon fires it during
/// teardown so we remove our item from the bus (before exit or a self-update
/// re-exec) without racing the single global waiter in `wait_for_shutdown`.
pub async fn run_supervised(
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
    tray_shutdown: Arc<Notify>,
) {
    // Initial best-effort registration, matching the pre-supervisor behavior.
    let mut handle = try_spawn(open_tx.clone(), refresh_tx.clone(), shutdown.clone()).await;
    if handle.is_some() {
        tracing::info!("system tray registered");
    }

    // Independent probe connection to the session bus, plus the owner we
    // registered against — seeded so the first tick doesn't spuriously rebuild a
    // perfectly good icon.
    let mut conn = zbus::Connection::session().await.ok();
    let mut prev_owner = match &conn {
        Some(c) => watcher_owner(c).await.ok().flatten(),
        None => None,
    };

    loop {
        tokio::select! {
            _ = tray_shutdown.notified() => break,
            _ = tokio::time::sleep(POLL_INTERVAL) => {}
        }

        // (Re)establish the probe connection if we lost it.
        if conn.is_none() {
            match zbus::Connection::session().await {
                Ok(c) => conn = Some(c),
                Err(e) => {
                    tracing::debug!("tray supervisor: no session bus ({e}); will retry");
                    continue;
                }
            }
        }

        let cur_owner = match watcher_owner(conn.as_ref().unwrap()).await {
            Ok(owner) => owner,
            Err(e) => {
                // Probe connection broke; drop it and reconnect next tick.
                tracing::debug!("tray supervisor: watcher probe failed ({e}); reconnecting");
                conn = None;
                continue;
            }
        };

        let have_live = handle.as_ref().is_some_and(|h| !h.is_closed());
        if tray_needs_rebuild(prev_owner.as_deref(), cur_owner.as_deref(), have_live) {
            if let Some(old) = handle.take() {
                old.shutdown().await;
            }
            handle = try_spawn(open_tx.clone(), refresh_tx.clone(), shutdown.clone()).await;
            if handle.is_some() {
                tracing::info!(
                    "system tray re-registered (SNI host {})",
                    cur_owner.as_deref().unwrap_or("?")
                );
            }
        }
        prev_owner = cur_owner;
    }

    // Clean teardown: remove our item from the bus so a stale icon doesn't
    // linger past the daemon exiting or re-execing onto a new binary.
    if let Some(h) = handle.take() {
        h.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use super::tray_needs_rebuild;

    #[test]
    fn no_host_never_rebuilds() {
        // Nothing to register with, whether or not we hold an item.
        assert!(!tray_needs_rebuild(None, None, false));
        assert!(!tray_needs_rebuild(Some(":1.10"), None, true));
    }

    #[test]
    fn host_present_without_live_tray_rebuilds() {
        // Host appeared (or we never registered) — register now.
        assert!(tray_needs_rebuild(None, Some(":1.10"), false));
        assert!(tray_needs_rebuild(Some(":1.10"), Some(":1.10"), false));
    }

    #[test]
    fn steady_state_does_not_flicker() {
        // Same host, live item — no rebuild, so the icon doesn't churn.
        assert!(!tray_needs_rebuild(Some(":1.10"), Some(":1.10"), true));
    }

    #[test]
    fn new_host_owner_rebuilds() {
        // Session relogin / shell restart: the watcher came back as a new owner
        // that never saw our item, so re-register even though we still hold a
        // (now-orphaned) handle.
        assert!(tray_needs_rebuild(Some(":1.10"), Some(":1.42"), true));
    }
}
