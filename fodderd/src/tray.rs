//! StatusNotifierItem system-tray icon via `ksni`.
//!
//! Registration is best-effort: on desktops without an SNI host (e.g. vanilla
//! GNOME Shell) `spawn` fails, and we degrade gracefully — the daemon keeps
//! polling and notifying, and the viewer can still be opened via a notification
//! click or by re-running `fodder`.

use std::sync::Arc;

use ksni::menu::{MenuItem, StandardItem};
use ksni::{Handle, Tray, TrayMethods};
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
}

impl Tray for FodderTray {
    fn id(&self) -> String {
        APP_ID.to_string()
    }

    fn title(&self) -> String {
        "Fodder Reader".to_string()
    }

    fn icon_name(&self) -> String {
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

/// Try to register the tray. Returns the handle on success (keep it alive for
/// the daemon's lifetime), or `None` if no SNI host is available.
pub async fn try_spawn(
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
