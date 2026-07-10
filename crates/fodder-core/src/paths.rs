use std::path::PathBuf;

use crate::ipc;

/// Data directory, `$FODDER_DATA_DIR` overrides for tests and daemons under test.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("FODDER_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fodder")
}

pub fn db_path() -> PathBuf {
    data_dir().join("fodder.db")
}

pub fn image_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("fodder")
        .join("images")
}

pub fn autostart_desktop_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("autostart")
        .join(format!("{}.Daemon.desktop", ipc::APP_ID))
}

const AUTOSTART_DESKTOP: &str = "[Desktop Entry]\n\
Type=Application\n\
Name=Fodder Feed Poller\n\
Comment=Background feed poller for Fodder\n\
Exec=fodder-daemon\n\
Icon=io.github.dipakmdhrm.Fodder\n\
NoDisplay=true\n\
X-GNOME-Autostart-enabled=true\n";

/// Create or remove the XDG autostart entry for the daemon.
pub fn set_autostart(enabled: bool) -> std::io::Result<()> {
    let path = autostart_desktop_path();
    if enabled {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, AUTOSTART_DESKTOP)
    } else {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

pub fn autostart_enabled() -> bool {
    autostart_desktop_path().exists()
}
