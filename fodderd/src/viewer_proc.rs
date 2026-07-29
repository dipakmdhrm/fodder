//! On-demand viewer process management.
//!
//! The daemon owns at most one `fodder` viewer child. Open requests either
//! route to an already-running viewer (over IPC) or spawn a new one. A reaper
//! task clears the tracked state when the child exits, and the child is killed
//! on daemon shutdown.

use std::path::PathBuf;

use fodder_core::ipc::IpcMessage;
use tokio::process::Command;
use tokio::sync::oneshot;

use crate::state::{AppCtx, OpenRequest};

/// Handle an open request: raise/navigate the existing viewer, or spawn one.
pub fn open(ctx: &AppCtx, req: OpenRequest) {
    // 1. A connected viewer can be raised/navigated directly over IPC.
    if ctx.send_to_viewer(message_for(&req)) {
        return;
    }
    // 2. A viewer was spawned but hasn't connected back yet — defer the request
    //    so it's delivered on `ViewerHello`.
    if ctx.viewer_alive() {
        *ctx.pending_open.lock().expect("pending_open poisoned") = Some(req);
        tracing::debug!("viewer starting; deferring open request");
        return;
    }
    // 3. No viewer at all — spawn one.
    spawn(ctx, req);
}

/// Deliver any deferred open request once the viewer has connected. Called from
/// the IPC server on `ViewerHello`.
pub fn deliver_pending(ctx: &AppCtx) {
    let pending = ctx.pending_open.lock().expect("pending_open poisoned").take();
    if let Some(req) = pending {
        ctx.send_to_viewer(message_for(&req));
    }
}

/// Ask the current viewer child (if any) to terminate. Used on daemon shutdown.
pub fn kill(ctx: &AppCtx) {
    if let Some(tx) = ctx.viewer_kill.lock().expect("viewer_kill poisoned").take() {
        let _ = tx.send(());
    }
}

/// The IPC message that raises/navigates the viewer for a request.
fn message_for(req: &OpenRequest) -> IpcMessage {
    match req {
        OpenRequest::Show => IpcMessage::OpenViewer,
        OpenRequest::At {
            feed_id,
            article_id,
        } => IpcMessage::OpenAt {
            feed_id: *feed_id,
            article_id: *article_id,
        },
    }
}

/// Spawn the viewer, passing the navigation target as CLI args, and start a
/// reaper task that clears tracked state when it exits.
fn spawn(ctx: &AppCtx, req: OpenRequest) {
    let exe = viewer_path();
    let mut cmd = Command::new(&exe);
    if let OpenRequest::At {
        feed_id,
        article_id,
    } = &req
    {
        cmd.arg("--feed").arg(feed_id.to_string());
        if let Some(a) = article_id {
            cmd.arg("--article").arg(a.to_string());
        }
    }
    // Reap ourselves rather than leaving a zombie.
    cmd.kill_on_drop(false);

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("failed to spawn viewer {}: {e}", exe.display());
            return;
        }
    };
    tracing::info!("spawned viewer (pid {:?})", child.id());
    ctx.set_viewer_alive(true);

    // Install a kill switch for shutdown.
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    *ctx.viewer_kill.lock().expect("viewer_kill poisoned") = Some(kill_tx);

    let ctx = ctx.clone();
    tokio::spawn(async move {
        tokio::select! {
            status = child.wait() => {
                tracing::info!("viewer exited: {status:?}");
            }
            _ = kill_rx => {
                let _ = child.kill().await;
                tracing::info!("viewer terminated on shutdown");
            }
        }
        // Clear all viewer-related state.
        ctx.set_viewer_alive(false);
        *ctx.viewer.lock().expect("viewer poisoned") = None;
        *ctx.pending_open.lock().expect("pending_open poisoned") = None;
        *ctx.viewer_kill.lock().expect("viewer_kill poisoned") = None;
    });
}

/// Locate the `fodder` viewer binary: next to the running daemon, else on PATH.
fn viewer_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("fodder");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("fodder")
}
