//! Thin gio-based D-Bus client for the daemon; also spawns it when absent.

use gtk4 as gtk;

use gtk::gio;

use fodder_core::ipc;

pub struct DaemonClient {
    conn: Option<gio::DBusConnection>,
}

impl DaemonClient {
    pub fn connect() -> Self {
        let conn = gio::bus_get_sync(gio::BusType::Session, None::<&gio::Cancellable>)
            .map_err(|e| log::warn!("no session bus, daemon coordination disabled: {e}"))
            .ok();
        DaemonClient { conn }
    }

    /// Start the daemon; if one is already running it exits on its own
    /// (bus-name single instance), so this is safe to call unconditionally.
    pub fn ensure_daemon_running(&self) {
        use std::os::unix::process::CommandExt;
        // Prefer a sibling binary (dev builds), fall back to PATH (installed).
        let sibling = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join(ipc::DAEMON_BIN)))
            .filter(|p| p.exists());
        let program = sibling.unwrap_or_else(|| ipc::DAEMON_BIN.into());
        let result = std::process::Command::new(program)
            .process_group(0)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        if let Err(e) = result {
            log::warn!("could not start {}: {e}", ipc::DAEMON_BIN);
        }
    }

    pub fn poll_now(&self) {
        self.call("PollNow");
    }

    pub fn quit_daemon(&self) {
        self.call("Quit");
    }

    fn call(&self, method: &str) {
        let Some(conn) = &self.conn else { return };
        let method_owned = method.to_string();
        conn.call(
            Some(ipc::DAEMON_BUS_NAME),
            ipc::DAEMON_OBJECT_PATH,
            ipc::DAEMON_INTERFACE,
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            3000,
            None::<&gio::Cancellable>,
            move |result| {
                if let Err(e) = result {
                    log::warn!("daemon call {method_owned} failed: {e}");
                }
            },
        );
    }

    /// Invoke `handler(new_count, unread_total)` on the main context whenever
    /// the daemon announces new items.
    pub fn on_items_added(&self, handler: impl Fn(u32, u32) + 'static) {
        let Some(conn) = &self.conn else { return };
        conn.signal_subscribe(
            Some(ipc::DAEMON_BUS_NAME),
            Some(ipc::DAEMON_INTERFACE),
            Some("ItemsAdded"),
            Some(ipc::DAEMON_OBJECT_PATH),
            None,
            gio::DBusSignalFlags::NONE,
            move |_conn, _sender, _path, _iface, _signal, params| {
                let new_count = params.child_value(0).get::<u32>().unwrap_or(0);
                let unread_total = params.child_value(1).get::<u32>().unwrap_or(0);
                handler(new_count, unread_total);
            },
        );
    }
}
