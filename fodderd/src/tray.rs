//! StatusNotifierItem system-tray icon via `ksni`.
//!
//! Registration is best-effort: on desktops without an SNI host (e.g. vanilla
//! GNOME Shell) `spawn` fails, and we degrade gracefully — the daemon keeps
//! polling and notifying, and the viewer can still be opened via a notification
//! click or by re-running `fodder`.
//!
//! **Recovery is event-driven**, like well-behaved SNI apps: `ksni` re-registers
//! the item whenever the tray watcher (`org.kde.StatusNotifierWatcher`) restarts
//! on the same bus, so a shell/watcher restart doesn't lose the icon. We do NOT
//! poll.
//!
//! The one wrinkle unique to fodder is `self_update`: on a package upgrade the
//! daemon re-execs itself (same PID), which releases and then immediately
//! re-requests the *same* well-known name `org.kde.StatusNotifierItem-<pid>-1`.
//! That handoff can race the watcher's prune-on-owner-vanished and drop us with
//! no further signal. So [`run`] does a single **deferred re-registration check**
//! shortly after startup: if our still-owned item isn't in the watcher's list,
//! re-register it once (a plain `RegisterStatusNotifierItem`, no tear-down). This
//! is a bounded one-shot, not a periodic poll.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Icon, Tray, TrayMethods};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::state::OpenRequest;

use fodder_core::APP_ID;

/// The well-known name the SNI host claims.
const WATCHER_NAME: &str = "org.kde.StatusNotifierWatcher";

/// Delay after spawning before the one-shot re-registration check, long enough
/// for a `self_update` re-exec handoff to have settled at the watcher.
const STARTUP_RECONCILE_DELAY: Duration = Duration::from_secs(5);

/// The tray model. Holds only cloneable senders so menu callbacks can hand work
/// off to the daemon without blocking the menu.
pub struct FodderTray {
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
    /// Embedded app icon as ARGB pixmaps; empty if decoding failed.
    icons: Vec<Icon>,
}

impl FodderTray {
    fn new(
        open_tx: UnboundedSender<OpenRequest>,
        refresh_tx: UnboundedSender<Option<i64>>,
        shutdown: Arc<Notify>,
    ) -> Self {
        Self {
            open_tx,
            refresh_tx,
            shutdown,
            icons: app_icons(),
        }
    }
}

impl Tray for FodderTray {
    fn id(&self) -> String {
        APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Fodder Reader".to_string()
    }

    fn icon_name(&self) -> String {
        // Prefer our embedded ARGB pixmaps (see `icon_pixmap`); only fall back
        // to the themed name if decoding them failed. Several SNI hosts (notably
        // the GNOME AppIndicator extension) prefer IconName and, when it doesn't
        // resolve in the icon theme, show a placeholder instead of the pixmap —
        // so we deliberately send an empty name when we have pixmaps.
        if self.icons.is_empty() {
            APP_ID.to_string()
        } else {
            String::new()
        }
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
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

/// Run the tray for the daemon's lifetime: spawn it, do a one-shot deferred
/// re-registration check (to neutralize the `self_update` re-exec handoff race),
/// then idle until `stop` is notified and tear the tray down. Ongoing recovery
/// across watcher restarts is `ksni`'s job — no polling here.
///
/// `quit` is the daemon-wide shutdown handle handed to the tray's Quit action;
/// `stop` is how `main` tells this task to exit.
pub async fn run(
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    quit: Arc<Notify>,
    stop: Arc<Notify>,
) {
    let handle = spawn(&open_tx, &refresh_tx, &quit).await;
    let has_tray = handle.is_some();

    // The one-shot reconcile, then idle forever (cancelled by `stop` below).
    let reconcile = async {
        if has_tray {
            tokio::time::sleep(STARTUP_RECONCILE_DELAY).await;
            reconcile_registration(std::process::id()).await;
        }
        std::future::pending::<()>().await;
    };

    tokio::select! {
        _ = stop.notified() => {}
        _ = reconcile => {}
    }

    if let Some(h) = handle {
        let _ = tokio::time::timeout(Duration::from_secs(2), h.shutdown()).await;
    }
}

/// Try to register the tray once. Returns the handle on success, or `None` if no
/// SNI host is available.
async fn spawn(
    open_tx: &UnboundedSender<OpenRequest>,
    refresh_tx: &UnboundedSender<Option<i64>>,
    quit: &Arc<Notify>,
) -> Option<Handle<FodderTray>> {
    let tray = FodderTray::new(open_tx.clone(), refresh_tx.clone(), quit.clone());
    match tray.spawn().await {
        Ok(handle) => {
            tracing::info!("system tray registered");
            Some(handle)
        }
        Err(e) => {
            tracing::warn!(
                "no system tray available ({e}); continuing without it \
                 (notifications and `fodder` re-run still open the viewer)"
            );
            None
        }
    }
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    fn register_status_notifier_item(&self, service: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
}

/// One-shot: if a watcher is up and our still-owned tray item isn't in its
/// registered list, re-register it once. This recovers from the re-exec handoff
/// race (old process released the same name we re-registered, and the watcher
/// pruned it). `ksni` still owns the name, so we just re-add it — no tear-down,
/// no fresh release/re-request, so no new race.
async fn reconcile_registration(our_pid: u32) {
    let Ok(conn) = zbus::Connection::session().await else {
        return;
    };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
        return;
    };
    // No SNI host present → nothing to register against.
    let Ok(watcher_bus) = zbus::names::BusName::try_from(WATCHER_NAME) else {
        return;
    };
    if !dbus.name_has_owner(watcher_bus).await.unwrap_or(false) {
        return;
    }
    let Ok(watcher) = StatusNotifierWatcherProxy::new(&conn).await else {
        return;
    };
    // The well-known name `ksni` registered for us (it embeds our PID).
    let Some(our_name) = our_item_name(&dbus, our_pid).await else {
        return; // ksni didn't register a well-known name; nothing to re-add.
    };
    let Ok(items) = watcher.registered_status_notifier_items().await else {
        return;
    };
    if items_include_pid(&dbus, &items, our_pid).await {
        return; // already registered — the common case; do nothing.
    }
    tracing::warn!(
        "tray item {our_name} missing from the watcher after startup \
         (likely a self_update re-exec handoff); re-registering"
    );
    if let Err(e) = watcher.register_status_notifier_item(&our_name).await {
        tracing::warn!("re-register failed: {e}");
    }
}

/// The `org.kde.StatusNotifierItem-<pid>-<n>` well-known name we own (its PID
/// segment is ours), or `None` if we don't own one.
async fn our_item_name(dbus: &zbus::fdo::DBusProxy<'_>, our_pid: u32) -> Option<String> {
    let prefix = format!("org.kde.StatusNotifierItem-{our_pid}-");
    let names = dbus.list_names().await.ok()?;
    names
        .into_iter()
        .find(|n| n.as_str().starts_with(&prefix))
        .map(|n| n.as_str().to_string())
}

/// Whether any of the watcher's registered entries is owned by our PID.
/// Host-agnostic: each entry is a bus name, optionally `@<objectpath>`-suffixed
/// (well-known on GNOME, unique on KDE); we resolve each to its owner PID.
async fn items_include_pid(
    dbus: &zbus::fdo::DBusProxy<'_>,
    items: &[String],
    our_pid: u32,
) -> bool {
    for entry in items {
        let Ok(name) = zbus::names::BusName::try_from(bus_name_of(entry)) else {
            continue;
        };
        if let Ok(pid) = dbus.get_connection_unix_process_id(name).await {
            if pid == our_pid {
                return true;
            }
        }
    }
    false
}

/// Strip the optional `@<objectpath>` suffix from a registered-item entry,
/// leaving just the bus name.
fn bus_name_of(entry: &str) -> &str {
    entry.split('@').next().unwrap_or(entry)
}

// --- icon embedding --------------------------------------------------------

/// The embedded app icon decoded into ARGB pixmaps at a few sizes, computed once.
fn app_icons() -> Vec<Icon> {
    static ICONS: OnceLock<Vec<Icon>> = OnceLock::new();
    ICONS
        .get_or_init(|| {
            const PNGS: &[&[u8]] = &[
                include_bytes!(
                    "../../data/icons/hicolor/24x24/apps/io.github.dipakmdhrm.Fodder.png"
                ),
                include_bytes!(
                    "../../data/icons/hicolor/32x32/apps/io.github.dipakmdhrm.Fodder.png"
                ),
                include_bytes!(
                    "../../data/icons/hicolor/48x48/apps/io.github.dipakmdhrm.Fodder.png"
                ),
            ];
            PNGS.iter().filter_map(|b| decode_png_argb(b)).collect()
        })
        .clone()
}

/// Decode a PNG into a ksni `Icon` (ARGB32, network byte order).
fn decode_png_argb(bytes: &[u8]) -> Option<Icon> {
    let mut decoder = png::Decoder::new(bytes);
    // Expand palette/grayscale/low-bit to 8-bit RGB(A); drop 16-bit to 8.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let pixels = &buf[..info.buffer_size()];
    let data = match info.color_type {
        png::ColorType::Rgba => rgba_to_argb(pixels),
        png::ColorType::Rgb => rgb_to_argb(pixels),
        _ => return None,
    };
    Some(Icon {
        width: info.width as i32,
        height: info.height as i32,
        data,
    })
}

/// RGBA8 → ARGB32 network byte order (bytes `[A, R, G, B]` per pixel).
fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        out.extend_from_slice(&[px[3], px[0], px[1], px[2]]);
    }
    out
}

/// RGB8 → ARGB32 network byte order, fully opaque.
fn rgb_to_argb(rgb: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgb.len() / 3 * 4);
    for px in rgb.chunks_exact(3) {
        out.extend_from_slice(&[0xff, px[0], px[1], px[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_name_of_strips_object_path() {
        assert_eq!(bus_name_of(":1.23@/org/foo"), ":1.23");
        assert_eq!(
            bus_name_of("org.kde.StatusNotifierItem-813993-1"),
            "org.kde.StatusNotifierItem-813993-1"
        );
    }

    #[test]
    fn rgba_to_argb_reorders_channels() {
        // one opaque red pixel [R,G,B,A] -> [A,R,G,B]
        assert_eq!(
            rgba_to_argb(&[0x11, 0x22, 0x33, 0xff]),
            vec![0xff, 0x11, 0x22, 0x33]
        );
    }

    #[test]
    fn rgb_to_argb_is_opaque() {
        assert_eq!(
            rgb_to_argb(&[0x11, 0x22, 0x33]),
            vec![0xff, 0x11, 0x22, 0x33]
        );
    }

    #[test]
    fn embedded_icons_decode() {
        // The bundled PNGs must actually decode, or we'd silently ship a
        // pixmap-less (placeholder-prone) tray.
        let icons = app_icons();
        assert!(!icons.is_empty(), "embedded app icons should decode");
        for icon in &icons {
            assert_eq!(icon.data.len(), (icon.width * icon.height * 4) as usize);
        }
    }
}
