//! Single-instance enforcement via the daemon's Unix socket.
//!
//! The socket at `$XDG_RUNTIME_DIR/fodder/daemon.sock` is both the IPC endpoint
//! and the "there can be only one daemon" guard. Binding it succeeds for the
//! first daemon; a would-be second daemon probes the existing socket to tell a
//! live daemon apart from a stale file left by a crash.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use fodder_core::ipc::{self, IpcMessage};
use tokio::net::{UnixListener, UnixStream};

/// Outcome of trying to become the daemon.
pub enum Acquired {
    /// We bound the socket and are now the sole daemon.
    Primary(UnixListener),
    /// A live daemon already owns the socket.
    AlreadyRunning,
}

/// Try to acquire the single-instance socket at `path`.
pub async fn acquire(path: &Path) -> Result<Acquired> {
    match UnixListener::bind(path) {
        Ok(listener) => Ok(Acquired::Primary(listener)),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            if probe_alive(path).await {
                Ok(Acquired::AlreadyRunning)
            } else {
                // Stale socket from a previous crash — remove and rebind.
                tracing::warn!("removing stale socket at {}", path.display());
                std::fs::remove_file(path)
                    .with_context(|| format!("removing stale socket {}", path.display()))?;
                let listener = UnixListener::bind(path)
                    .with_context(|| format!("binding socket {}", path.display()))?;
                Ok(Acquired::Primary(listener))
            }
        }
        Err(e) => Err(e).with_context(|| format!("binding socket {}", path.display())),
    }
}

/// Probe whether a live daemon answers on the socket: connect, send `Ping`, and
/// wait briefly for `Pong`. Any failure means "not alive" (stale socket).
async fn probe_alive(path: &Path) -> bool {
    let Ok(mut stream) = UnixStream::connect(path).await else {
        return false;
    };
    if ipc::write_msg(&mut stream, &IpcMessage::Ping)
        .await
        .is_err()
    {
        return false;
    }
    matches!(
        tokio::time::timeout(Duration::from_secs(2), ipc::read_msg(&mut stream)).await,
        Ok(Ok(Some(IpcMessage::Pong)))
    )
}

/// Connect to a running daemon and ask it to open/raise the viewer, then close.
/// Used by a second `fodderd` invocation.
pub async fn request_open(path: &Path) -> Result<()> {
    let mut stream = UnixStream::connect(path)
        .await
        .with_context(|| format!("connecting to daemon at {}", path.display()))?;
    ipc::write_msg(&mut stream, &IpcMessage::OpenViewer).await?;
    // Best-effort: wait for the Ack so the daemon has processed it.
    let _ = tokio::time::timeout(Duration::from_secs(2), ipc::read_msg(&mut stream)).await;
    Ok(())
}
