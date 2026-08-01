//! Autostart integration.
//!
//! On a **native** install this writes/removes `~/.config/autostart/fodder.desktop`,
//! which launches the daemon (`fodderd`) at login; the `.desktop` file on disk is
//! the source of truth for whether autostart is enabled.
//!
//! Inside a **Flatpak sandbox** that path is the app's private, host-invisible
//! `~/.var/app/.../config/autostart`, so writing it would do nothing. There,
//! autostart-on-login must instead be requested through the XDG **Background**
//! portal (`org.freedesktop.portal.Background.RequestBackground`). The portal
//! call is async D-Bus and lives in the daemon (`fodderd`, which already has
//! `zbus` + tokio); this module supplies the pure decision helpers it needs plus
//! a marker file both sandboxed processes read to reflect the current intent
//! (the portal exposes no "is autostart enabled?" query).
//!
//! Shared by the daemon and by the viewer's settings UI.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::paths;

/// Whether we're running inside a Flatpak sandbox. The runtime bind-mounts
/// `/.flatpak-info` into every sandbox, so its presence is the canonical probe.
pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

/// The in-sandbox command the Background portal should register as the login
/// autostart entry: start the daemon **headless** (tray + polling, no viewer),
/// mirroring the native `Exec=fodderd` entry. `xdg-desktop-portal` wraps this in
/// the appropriate `flatpak run` invocation when it writes the host autostart file.
pub fn portal_autostart_command() -> Vec<String> {
    vec!["fodderd".to_string()]
}

/// Path to the Flatpak autostart-intent marker (a zero-byte file under the
/// config dir). The Background portal has no getter, so the daemon touches this
/// on a granted request and removes it on disable; the viewer reads it to seed
/// the settings toggle. Native installs use [`desktop_path`] instead.
pub fn flatpak_marker_path() -> Result<PathBuf> {
    Ok(paths::config_dir()?.join("autostart-requested"))
}

/// Path to the autostart entry.
pub fn desktop_path() -> Result<PathBuf> {
    paths::autostart_desktop_path()
}

/// Whether autostart is currently enabled: the marker under Flatpak, else the
/// presence of the native autostart `.desktop` file.
pub fn is_enabled() -> bool {
    if is_flatpak() {
        return flatpak_marker_path().map(|p| p.exists()).unwrap_or(false);
    }
    desktop_path().map(|p| p.exists()).unwrap_or(false)
}

/// Record the Flatpak autostart intent by creating/removing the marker file.
/// Called by the daemon after the Background portal request resolves.
pub fn set_flatpak_marker(enabled: bool) -> Result<()> {
    let path = flatpak_marker_path()?;
    if enabled {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, b"")?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_command_starts_daemon_headless() {
        // The login autostart must bring up the daemon (tray + polling) WITHOUT
        // popping the viewer, matching the native `Exec=fodderd` entry.
        let cmd = portal_autostart_command();
        assert_eq!(cmd, vec!["fodderd".to_string()]);
        assert!(
            !cmd.iter().any(|a| a.contains("open-viewer")),
            "autostart must not open the viewer on every login"
        );
    }

    #[test]
    fn flatpak_marker_lives_under_config_dir() {
        // The marker must sit in the config dir (shared by daemon and viewer in
        // the same sandbox) and be named distinctly from the native entry.
        let marker = flatpak_marker_path().unwrap();
        assert_eq!(marker.parent().unwrap(), paths::config_dir().unwrap());
        assert_eq!(
            marker.file_name().and_then(|n| n.to_str()),
            Some("autostart-requested")
        );
    }
}
