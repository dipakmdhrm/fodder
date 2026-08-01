//! StatusNotifierItem system-tray icon via `ksni`.
//!
//! Kept deliberately simple by tying the tray to the *session* lifecycle instead
//! of trying to keep it alive across sessions:
//!
//! - We register with `disable_dbus_name(true)`, so the item uses `ksni`'s unique
//!   D-Bus connection name rather than a well-known `org.kde.StatusNotifierItem-…`
//!   name. That works inside Flatpak (which can't own that name) and means a
//!   restarted daemon never collides with a previous registration.
//! - `assume_sni_available(true)` lets us autostart *before* the desktop's tray
//!   host is up: `ksni` keeps the item and registers once the watcher appears.
//! - When the D-Bus *session* connection dies — which is what a logout does — the
//!   daemon shuts down (see [`wait_for_session_end`], wired up in `main`), so the
//!   next login's autostart brings up a fresh daemon on a fresh connection with a
//!   working tray. No in-daemon re-registration/reconnection logic needed.
//!
//! Registration is still best-effort: on a host with no SNI tray the daemon keeps
//! polling and notifying, and the viewer opens via a notification or `fodder`.

use std::sync::{Arc, OnceLock};

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Icon, Tray, TrayMethods};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

use crate::state::OpenRequest;

use fodder_core::APP_ID;

/// The tray model. Holds only cloneable senders so menu callbacks can hand work
/// off to the daemon without blocking the menu.
pub struct FodderTray {
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
    /// Embedded app icon as ARGB pixmaps; empty if decoding failed.
    icons: Vec<Icon>,
}

impl Tray for FodderTray {
    fn id(&self) -> String {
        APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Fodder Reader".to_string()
    }

    fn icon_name(&self) -> String {
        // Prefer our embedded ARGB pixmaps; only fall back to the themed name if
        // decoding failed. Several hosts (notably the GNOME AppIndicator
        // extension) prefer IconName and show a placeholder when it doesn't
        // resolve in the theme, so we send an empty name when we have pixmaps.
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

/// Register the tray. Returns the handle (keep it alive for the daemon's
/// lifetime, and `shutdown()` it on exit), or `None` if the D-Bus connection
/// couldn't be established.
pub async fn spawn(
    open_tx: UnboundedSender<OpenRequest>,
    refresh_tx: UnboundedSender<Option<i64>>,
    shutdown: Arc<Notify>,
) -> Option<Handle<FodderTray>> {
    let tray = FodderTray {
        open_tx,
        refresh_tx,
        shutdown,
        icons: app_icons(),
    };
    match tray
        .disable_dbus_name(true)
        .assume_sni_available(true)
        .spawn()
        .await
    {
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

/// Resolve when the D-Bus *session* connection ends — which is what happens on
/// logout (the socket is severed). `main` uses this as a shutdown trigger so the
/// daemon exits with the session and the next login starts a fresh one. If there
/// is no session bus at all (headless), never resolves.
pub async fn wait_for_session_end() {
    match zbus::Connection::session().await {
        Ok(conn) => conn.closed().await,
        Err(_) => std::future::pending().await,
    }
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
    fn rgba_to_argb_reorders_channels() {
        // one opaque pixel [R,G,B,A] -> [A,R,G,B]
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
