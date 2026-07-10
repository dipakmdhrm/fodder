use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

use fodder_core::ipc;

/// Launch the viewer, fully detached. If one is already running, its
/// GApplication single-instance handling just presents the existing window.
pub fn spawn_viewer() {
    let result = Command::new(ipc::VIEWER_BIN)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = result {
        log::warn!("failed to launch viewer: {e}");
    }
}
