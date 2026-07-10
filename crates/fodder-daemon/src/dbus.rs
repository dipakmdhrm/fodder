use std::sync::atomic::Ordering;
use std::sync::Arc;

use fodder_core::ipc;

use crate::Shared;

pub enum ServeError {
    /// Another daemon instance already owns the bus name.
    NameTaken,
    Other(zbus::Error),
}

/// Test-only override so integration tests don't collide with a real daemon.
fn bus_name() -> String {
    std::env::var("FODDER_BUS_NAME").unwrap_or_else(|_| ipc::DAEMON_BUS_NAME.to_string())
}

struct DaemonIface {
    shared: Arc<Shared>,
}

#[zbus::interface(name = "io.github.dipakmdhrm.Fodder.Daemon1")]
impl DaemonIface {
    fn poll_now(&self) {
        self.shared.signal(|f| f.poll_now = true);
    }

    fn settings_changed(&self) {
        self.shared.signal(|f| f.settings_changed = true);
    }

    fn quit(&self) {
        self.shared.signal(|f| f.quit = true);
    }

    #[zbus(property)]
    fn unread_total(&self) -> u32 {
        self.shared.unread.load(Ordering::SeqCst)
    }
}

pub fn serve(shared: Arc<Shared>) -> Result<zbus::blocking::Connection, ServeError> {
    let build = || -> zbus::Result<zbus::blocking::Connection> {
        zbus::blocking::connection::Builder::session()?
            .name(bus_name().as_str())?
            .serve_at(ipc::DAEMON_OBJECT_PATH, DaemonIface { shared })?
            .build()
    };
    build().map_err(|e| match e {
        zbus::Error::NameTaken => ServeError::NameTaken,
        other => ServeError::Other(other),
    })
}

pub fn emit_items_added(conn: &zbus::blocking::Connection, new_count: u32, unread_total: u32) {
    let result = zbus::block_on(conn.inner().emit_signal(
        Option::<zbus::names::BusName>::None,
        ipc::DAEMON_OBJECT_PATH,
        ipc::DAEMON_INTERFACE,
        "ItemsAdded",
        &(new_count, unread_total),
    ));
    if let Err(e) = result {
        log::warn!("failed to emit ItemsAdded: {e}");
    }
}
