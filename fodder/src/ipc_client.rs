//! Viewer-side IPC client.
//!
//! Runs on the tokio runtime (async I/O needs the reactor). It connects to the
//! daemon, announces itself with `ViewerHello`, forwards inbound daemon messages
//! to the GTK thread as [`FromDaemon`] events, and writes outbound commands
//! (refresh, subscribe) that the UI enqueues.

use fodder_core::ipc::{self, IpcMessage};
use fodder_core::paths;
use tokio::net::UnixStream;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

/// Events delivered from the daemon to the GTK main thread.
#[derive(Debug)]
pub enum FromDaemon {
    /// Raise/show the window.
    Open,
    /// Navigate to a feed / article.
    OpenAt {
        feed_id: i64,
        article_id: Option<i64>,
    },
    /// The feed set or articles changed; reload from the DB.
    FeedsChanged,
    /// The daemon says another viewer already owns the session; we should exit.
    Duplicate,
    /// The connection to the daemon was lost (or never established).
    Disconnected,
}

/// Start the IPC client task. Returns a sender for outbound commands to the
/// daemon; inbound events arrive on `ui_tx`. If the daemon can't be reached the
/// task sends [`FromDaemon::Disconnected`] and exits, but the returned sender
/// stays valid (its messages are simply dropped) so the UI still runs.
pub fn start(
    handle: &tokio::runtime::Handle,
    ui_tx: UnboundedSender<FromDaemon>,
) -> UnboundedSender<IpcMessage> {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<IpcMessage>();
    handle.spawn(async move {
        if let Err(()) = run(ui_tx.clone(), cmd_rx).await {
            let _ = ui_tx.send(FromDaemon::Disconnected);
        }
    });
    cmd_tx
}

async fn run(
    ui_tx: UnboundedSender<FromDaemon>,
    mut cmd_rx: UnboundedReceiver<IpcMessage>,
) -> Result<(), ()> {
    let path = paths::daemon_socket_path().map_err(|_| ())?;
    let stream = UnixStream::connect(&path).await.map_err(|_| ())?;
    let (mut reader, mut writer) = stream.into_split();

    ipc::write_msg(&mut writer, &IpcMessage::ViewerHello)
        .await
        .map_err(|_| ())?;

    loop {
        tokio::select! {
            inbound = ipc::read_msg(&mut reader) => {
                match inbound {
                    Ok(Some(msg)) => {
                        if let Some(event) = classify(msg) {
                            if ui_tx.send(event).is_err() {
                                break; // UI gone
                            }
                        }
                    }
                    _ => {
                        let _ = ui_tx.send(FromDaemon::Disconnected);
                        break;
                    }
                }
            }
            outbound = cmd_rx.recv() => {
                match outbound {
                    Some(msg) => {
                        if ipc::write_msg(&mut writer, &msg).await.is_err() {
                            let _ = ui_tx.send(FromDaemon::Disconnected);
                            break;
                        }
                    }
                    None => break, // UI dropped the command sender
                }
            }
        }
    }
    Ok(())
}

/// Map a daemon message to a UI event, or `None` to ignore it.
fn classify(msg: IpcMessage) -> Option<FromDaemon> {
    match msg {
        IpcMessage::OpenViewer => Some(FromDaemon::Open),
        IpcMessage::OpenAt {
            feed_id,
            article_id,
        } => Some(FromDaemon::OpenAt {
            feed_id,
            article_id,
        }),
        IpcMessage::FeedsChanged => Some(FromDaemon::FeedsChanged),
        // An error reply to our ViewerHello means a viewer already exists.
        IpcMessage::Error(_) => Some(FromDaemon::Duplicate),
        _ => None,
    }
}
