//! D-Bus names shared by the daemon (service) and the viewer (client).
//!
//! Interface `io.github.dipakmdhrm.Fodder.Daemon1`:
//! - Methods: `PollNow()`, `SettingsChanged()`, `Quit()`
//! - Signal: `ItemsAdded(new_count: u32, unread_total: u32)`
//! - Property: `UnreadTotal: u32`

pub const APP_ID: &str = "io.github.dipakmdhrm.Fodder";
pub const DAEMON_BUS_NAME: &str = "io.github.dipakmdhrm.Fodder.Daemon";
pub const DAEMON_OBJECT_PATH: &str = "/io/github/dipakmdhrm/Fodder/Daemon";
pub const DAEMON_INTERFACE: &str = "io.github.dipakmdhrm.Fodder.Daemon1";

pub const VIEWER_BIN: &str = "fodder";
pub const DAEMON_BIN: &str = "fodder-daemon";
