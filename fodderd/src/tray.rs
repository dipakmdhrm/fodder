//! StatusNotifierItem system-tray icon via `ksni`, with a self-heal supervisor.
//!
//! Registration is best-effort: on desktops without an SNI host (e.g. vanilla
//! GNOME Shell) `spawn` fails, and we degrade gracefully — the daemon keeps
//! polling and notifying, and the viewer can still be opened via a notification
//! click or by re-running `fodder`.
//!
//! **Why the supervisor.** The tray is registered once, on the D-Bus session
//! connection `ksni` opens. `ksni` re-registers when the tray *watcher*
//! (`org.kde.StatusNotifierWatcher`) restarts on the same bus — but in practice
//! that event-driven recovery can still miss a drop (observed live on GNOME:
//! the daemon kept running and owned its item name, yet was no longer in the
//! watcher's registered list, so its icon was gone). [`supervise`] adds the
//! belt-and-suspenders the working reference implementations use: it
//! periodically checks whether *our* process is still among the watcher's
//! registered items and, if not, re-registers by re-spawning the tray. This is
//! cause-agnostic — it recovers from any drop, not just a clean watcher cycle.

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

/// How often the supervisor checks that our tray is still registered.
const CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Consecutive "missing" checks (while we believe we have a tray) before we
/// re-register. A small buffer so a freshly-spawned item that hasn't propagated
/// to the watcher yet doesn't trigger a needless re-spawn.
const MISSES_BEFORE_RESPAWN: u32 = 2;

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

/// Run the tray for the daemon's lifetime: spawn it, then periodically make sure
/// it's still registered with the watcher and re-register if it was dropped.
/// Returns when `stop` is notified (daemon shutdown), tearing the tray down.
///
/// `quit` is the daemon-wide shutdown handle handed to the tray's Quit action;
/// `stop` is how `main` tells this supervisor to exit.
pub async fn supervise(
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    quit: Arc<Notify>,
    stop: Arc<Notify>,
) {
    let our_pid = std::process::id();
    let mut handle = spawn(&open_tx, &refresh_tx, &quit).await;

    let mut ticker = tokio::time::interval(CHECK_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut misses: u32 = 0;

    loop {
        tokio::select! {
            _ = stop.notified() => break,
            _ = ticker.tick() => {
                match registration_status(our_pid).await {
                    // Registered, or we can't tell (no watcher / bus hiccup):
                    // reset and do nothing.
                    RegStatus::Registered | RegStatus::Unknown => misses = 0,
                    RegStatus::Missing => {
                        if handle.is_none() {
                            // No tray yet (initial spawn failed, e.g. the host
                            // wasn't up). A watcher exists now — try again.
                            handle = spawn(&open_tx, &refresh_tx, &quit).await;
                            misses = 0;
                        } else {
                            misses += 1;
                            if misses >= MISSES_BEFORE_RESPAWN {
                                tracing::warn!(
                                    "tray no longer registered with the StatusNotifierWatcher; \
                                     re-registering"
                                );
                                if let Some(h) = handle.take() {
                                    let _ = tokio::time::timeout(
                                        Duration::from_secs(2), h.shutdown()).await;
                                }
                                handle = spawn(&open_tx, &refresh_tx, &quit).await;
                                misses = 0;
                            }
                        }
                    }
                }
            }
        }
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

/// Whether *this* process's tray item is currently registered with the watcher.
enum RegStatus {
    /// Our PID owns one of the watcher's registered items.
    Registered,
    /// A watcher exists, but none of its registered items belongs to us.
    Missing,
    /// No watcher, or we couldn't query it — don't act on this.
    Unknown,
}

#[zbus::proxy(
    interface = "org.kde.StatusNotifierWatcher",
    default_service = "org.kde.StatusNotifierWatcher",
    default_path = "/StatusNotifierWatcher"
)]
trait StatusNotifierWatcher {
    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> zbus::Result<Vec<String>>;
}

/// Query the watcher and decide whether our tray item is still registered.
///
/// Each registered entry is a bus name, optionally suffixed with `@<objectpath>`
/// (`org.kde.StatusNotifierItem-<pid>-<n>` or `:1.23@/org/foo`). We resolve each
/// to its owner PID and compare with ours — host-agnostic, so it works whether
/// the watcher stores the well-known name (GNOME) or the unique name (KDE).
async fn registration_status(our_pid: u32) -> RegStatus {
    let Ok(conn) = zbus::Connection::session().await else {
        return RegStatus::Unknown;
    };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else {
        return RegStatus::Unknown;
    };
    // No SNI host present → nothing to (re)register against; leave it alone so
    // we don't churn on desktops that simply have no tray.
    let Ok(watcher_name) = zbus::names::BusName::try_from(WATCHER_NAME) else {
        return RegStatus::Unknown;
    };
    if !dbus.name_has_owner(watcher_name).await.unwrap_or(false) {
        return RegStatus::Unknown;
    }

    let Ok(watcher) = StatusNotifierWatcherProxy::new(&conn).await else {
        return RegStatus::Unknown;
    };
    let Ok(items) = watcher.registered_status_notifier_items().await else {
        return RegStatus::Unknown;
    };

    for entry in &items {
        let Ok(name) = zbus::names::BusName::try_from(bus_name_of(entry)) else {
            continue;
        };
        if let Ok(pid) = dbus.get_connection_unix_process_id(name).await {
            if pid == our_pid {
                return RegStatus::Registered;
            }
        }
    }
    RegStatus::Missing
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
