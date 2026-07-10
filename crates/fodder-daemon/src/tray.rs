use std::sync::Arc;

use ksni::menu::{MenuItem, StandardItem};

use crate::{util, Shared};

pub struct FodderTray {
    unread: u32,
    shared: Arc<Shared>,
}

impl ksni::Tray for FodderTray {
    fn id(&self) -> String {
        fodder_core::ipc::APP_ID.into()
    }

    fn title(&self) -> String {
        if self.unread > 0 {
            format!("Fodder — {} unread", self.unread)
        } else {
            "Fodder".into()
        }
    }

    fn icon_name(&self) -> String {
        // Themed fallback until our own icon ships with the packages.
        "application-rss+xml-symbolic".into()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        util::spawn_viewer();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: "Open Fodder".into(),
                activate: Box::new(|_: &mut Self| util::spawn_viewer()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Poll now".into(),
                activate: Box::new(|tray: &mut Self| tray.shared.signal(|f| f.poll_now = true)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                activate: Box::new(|tray: &mut Self| tray.shared.signal(|f| f.quit = true)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Owns the (optional) tray service; the tray only exists while
/// "run in background" is enabled, since its purpose is reopening the viewer.
pub struct TrayState {
    handle: Option<ksni::Handle<FodderTray>>,
    shared: Arc<Shared>,
}

impl TrayState {
    pub fn new(shared: Arc<Shared>) -> Self {
        TrayState {
            handle: None,
            shared,
        }
    }

    pub fn set_visible(&mut self, visible: bool, unread: u32) {
        match (visible, self.handle.take()) {
            (true, None) => {
                let service = ksni::TrayService::new(FodderTray {
                    unread,
                    shared: self.shared.clone(),
                });
                let handle = service.handle();
                // Registration failure (no SNI host, e.g. vanilla GNOME) is
                // non-fatal: the service thread logs and exits.
                service.spawn();
                self.handle = Some(handle);
            }
            (false, Some(handle)) => handle.shutdown(),
            (_, existing) => self.handle = existing,
        }
    }

    pub fn set_unread(&self, unread: u32) {
        if let Some(handle) = &self.handle {
            handle.update(|tray| tray.unread = unread);
        }
    }
}
