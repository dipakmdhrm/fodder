//! Autostart integration: writes/removes `~/.config/autostart/fodder.desktop`,
//! which launches the daemon (`fodderd`) at login.
//!
//! The `.desktop` file on disk is the source of truth for whether autostart is
//! enabled. Shared by the daemon and by the viewer's settings UI.

use std::path::PathBuf;

use anyhow::Result;

use crate::paths;

/// Path to the autostart entry.
pub fn desktop_path() -> Result<PathBuf> {
    paths::autostart_desktop_path()
}

/// Whether the autostart entry currently exists.
pub fn is_enabled() -> bool {
    desktop_path().map(|p| p.exists()).unwrap_or(false)
}

/// Install the autostart entry, launching the resolved `fodderd` binary.
pub fn enable() -> Result<()> {
    let exec = daemon_exec();
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Fodder Reader\n\
         Comment=RSS/Atom/JSON feed reader daemon\n\
         Exec={exec}\n\
         Icon=io.github.dipakmdhrm.Fodder\n\
         Terminal=false\n\
         Categories=Network;News;\n\
         X-GNOME-Autostart-enabled=true\n"
    );
    let path = desktop_path()?;
    std::fs::write(&path, content)?;
    tracing::info!("autostart enabled: {}", path.display());
    Ok(())
}

/// Remove the autostart entry, if present.
pub fn disable() -> Result<()> {
    let path = desktop_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::info!("autostart disabled: {}", path.display());
    }
    Ok(())
}

/// Enable or disable to match `enabled`.
pub fn set_enabled(enabled: bool) -> Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

/// Resolve the `fodderd` command for the `Exec=` line. Prefer an absolute path
/// next to the current executable (works whether called from `fodderd` itself
/// or from the `fodder` viewer, which sits in the same directory); fall back to
/// the bare name so a PATH-installed binary still works.
fn daemon_exec() -> String {
    if let Ok(exe) = std::env::current_exe() {
        if exe.file_name().and_then(|n| n.to_str()) == Some("fodderd") {
            return exe.display().to_string();
        }
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("fodderd");
            if candidate.exists() {
                return candidate.display().to_string();
            }
        }
    }
    "fodderd".to_string()
}
