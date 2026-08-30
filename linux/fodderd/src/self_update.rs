//! Self-restart when the daemon binary is replaced in place.
//!
//! When a package upgrade (`apt upgrade`, `dnf upgrade`, `pacman -Syu`) swaps
//! out the installed `fodderd` binary underneath the running daemon, the old
//! process keeps executing the old code (Linux keeps the old inode mapped), so
//! its tray/notifications would otherwise linger on the old version until the
//! next login. Instead we notice the on-disk binary changed and re-exec the new
//! one in place, so the daemon and tray come back on the new version within a
//! few seconds -- same session, same environment, no root or systemd needed.
//!
//! Only the plain metadata comparison is unit-tested here; the watch loop and
//! the `exec` handoff are platform glue exercised manually.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;

/// How often to check whether the on-disk binary changed. A single `stat` at
/// this cadence is negligible next to the feed poll loop.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Grace period after a change is first seen, so the upgrade (both binaries
/// plus any maintainer scripts) settles before we hand off.
const SETTLE: Duration = Duration::from_secs(3);

/// Identity of the binary file on disk, used to detect in-place replacement.
/// A package upgrade unlinks the old file and links a new one, changing the
/// inode (and typically size/mtime), so any field differing means "replaced".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BinarySignature {
    dev: u64,
    ino: u64,
    size: u64,
    mtime: i64,
}

impl BinarySignature {
    /// Read the current signature of `path`, or `None` if it can't be stat'd.
    pub fn read(path: &Path) -> Option<Self> {
        use std::os::unix::fs::MetadataExt;
        let m = std::fs::metadata(path).ok()?;
        Some(Self {
            dev: m.dev(),
            ino: m.ino(),
            size: m.size(),
            mtime: m.mtime(),
        })
    }
}

/// Poll `exe_path`; once its signature changes, flag `reexec` and wake
/// `shutdown` so `main` tears everything down cleanly and re-execs. Returns
/// after triggering (or immediately if the path can't be read at startup).
pub async fn watch(exe_path: PathBuf, reexec: Arc<AtomicBool>, shutdown: Arc<Notify>) {
    let Some(baseline) = BinarySignature::read(&exe_path) else {
        tracing::debug!(
            "self-update: cannot stat {}; watcher disabled",
            exe_path.display()
        );
        return;
    };

    let mut ticker = tokio::time::interval(POLL_INTERVAL);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        ticker.tick().await;
        if let Some(cur) = BinarySignature::read(&exe_path) {
            if cur != baseline {
                tracing::info!("fodderd binary replaced on disk; restarting onto the new version");
                tokio::time::sleep(SETTLE).await;
                reexec.store(true, Ordering::SeqCst);
                shutdown.notify_one();
                return;
            }
        }
    }
}

/// Replace the current process image with a fresh, bare (headless) `fodderd`.
/// Passes no arguments so the daemon comes back resident with just the tray.
/// Only returns (as an error) if `exec` itself fails.
pub fn reexec_into(exe_path: &Path) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(exe_path).exec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_detects_replacement() {
        let dir = std::env::temp_dir().join(format!("fodder-selfupd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("bin");

        std::fs::write(&f, b"old").unwrap();
        let before = BinarySignature::read(&f).expect("signature before");

        // Rewrite with different-length content so the size differs regardless
        // of mtime clock resolution -- a deterministic "replaced" signal.
        std::fs::write(&f, b"a-much-longer-new-binary-image").unwrap();
        let after = BinarySignature::read(&f).expect("signature after");

        assert_ne!(before, after, "replacement must change the signature");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn signature_missing_path_is_none() {
        assert!(BinarySignature::read(Path::new("/no/such/fodderd/binary")).is_none());
    }
}
