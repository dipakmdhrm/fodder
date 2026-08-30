//! Filesystem and runtime paths, derived from XDG via `directories`.
//!
//! Config:  `~/.config/fodder/config.toml`
//! Data:    `~/.local/share/fodder/db.sqlite`
//! Runtime: `$XDG_RUNTIME_DIR/fodder/daemon.sock` (single-instance + IPC)
//! Autostart: `~/.config/autostart/fodder.desktop`

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;

const QUALIFIER: &str = "io.github";
const ORGANIZATION: &str = "dipakmdhrm";
const APPLICATION: &str = "Fodder";

fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .ok_or_else(|| anyhow!("could not determine XDG base directories"))
}

/// `~/.config/fodder/` — created if missing.
pub fn config_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating config dir {}", dir.display()))?;
    Ok(dir)
}

/// `~/.config/fodder/config.toml`.
pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

/// `~/.local/share/fodder/` — created if missing.
pub fn data_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data dir {}", dir.display()))?;
    Ok(dir)
}

/// `~/.local/share/fodder/db.sqlite`.
pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("db.sqlite"))
}

/// `$XDG_RUNTIME_DIR/fodder/` — created if missing. Falls back to
/// `/run/user/<uid>` and finally the system temp dir if unset.
pub fn runtime_dir() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            // Fallback: /run/user/<uid> if it exists.
            let uid = unsafe { libc_getuid() };
            let p = PathBuf::from(format!("/run/user/{uid}"));
            p.is_dir().then_some(p)
        })
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("fodder");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating runtime dir {}", dir.display()))?;
    Ok(dir)
}

/// `$XDG_RUNTIME_DIR/fodder/daemon.sock` — the single-instance guard and
/// daemon↔viewer IPC endpoint.
pub fn daemon_socket_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("daemon.sock"))
}

/// `~/.config/autostart/fodder.desktop` — written/removed by the autostart
/// toggle in settings.
pub fn autostart_desktop_path() -> Result<PathBuf> {
    // The autostart dir is a sibling of the app config dir under ~/.config.
    let dirs = project_dirs()?;
    let config_root = dirs
        .config_dir()
        .parent()
        .ok_or_else(|| anyhow!("config dir has no parent"))?
        .to_path_buf();
    let dir = config_root.join("autostart");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating autostart dir {}", dir.display()))?;
    Ok(dir.join("fodder.desktop"))
}

/// Minimal `getuid()` binding so we avoid a dependency just for the runtime-dir
/// fallback path.
unsafe fn libc_getuid() -> u32 {
    extern "C" {
        fn getuid() -> u32;
    }
    getuid()
}
