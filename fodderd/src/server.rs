//! IPC server: accepts viewer / second-instance connections and dispatches
//! messages against the shared [`AppCtx`].

use fodder_core::db::feeds;
use fodder_core::ipc::{self, IpcMessage};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::state::{AppCtx, OpenRequest};

/// Accept connections forever, handling each on its own task.
pub async fn run(listener: UnixListener, ctx: AppCtx) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let ctx = ctx.clone();
                tokio::spawn(handle_conn(stream, ctx));
            }
            Err(e) => {
                tracing::warn!("accept failed: {e}");
            }
        }
    }
}

/// Handle one connection. A connection becomes "the viewer" if it sends
/// `ViewerHello`; its outbound queue is then registered in `ctx.viewer` so
/// other tasks can push `OpenAt` / `FeedsChanged` to it.
async fn handle_conn(stream: UnixStream, ctx: AppCtx) {
    let (mut reader, writer) = stream.into_split();

    // A dedicated writer task drains the outbound queue to the socket, so both
    // the read loop and other tasks can enqueue messages concurrently.
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<IpcMessage>();
    let writer_task = tokio::spawn(async move {
        let mut writer = writer;
        while let Some(msg) = out_rx.recv().await {
            if ipc::write_msg(&mut writer, &msg).await.is_err() {
                break;
            }
        }
    });

    let mut is_viewer = false;

    loop {
        match ipc::read_msg(&mut reader).await {
            Ok(Some(msg)) => {
                if handle_msg(msg, &ctx, &out_tx, &mut is_viewer).await {
                    break; // peer asked to close
                }
            }
            Ok(None) => break, // clean EOF
            Err(e) => {
                tracing::debug!("ipc read error: {e}");
                break;
            }
        }
    }

    // Deregister the viewer if this was it.
    if is_viewer {
        let mut guard = ctx.viewer.lock().expect("viewer mutex poisoned");
        *guard = None;
        tracing::info!("viewer disconnected");
    }
    drop(out_tx);
    let _ = writer_task.await;
}

/// Process one message. Returns `true` if the connection should close.
async fn handle_msg(
    msg: IpcMessage,
    ctx: &AppCtx,
    out_tx: &mpsc::UnboundedSender<IpcMessage>,
    is_viewer: &mut bool,
) -> bool {
    match msg {
        IpcMessage::Ping => {
            let _ = out_tx.send(IpcMessage::Pong);
        }
        IpcMessage::ViewerHello => {
            let already_connected = ctx.viewer.lock().expect("viewer mutex poisoned").is_some();
            if already_connected {
                // Enforce exactly one viewer: raise the existing one and tell
                // this duplicate to exit.
                tracing::info!("duplicate viewer rejected; raising the existing one");
                ctx.send_to_viewer(IpcMessage::OpenViewer);
                let _ = out_tx.send(IpcMessage::Error("viewer already running".into()));
                return true;
            }
            *is_viewer = true;
            *ctx.viewer.lock().expect("viewer mutex poisoned") = Some(out_tx.clone());
            ctx.set_viewer_alive(true);
            tracing::info!("viewer connected");
            let _ = out_tx.send(IpcMessage::Ack);
            // Deliver any open request that was deferred while it started up.
            crate::viewer_proc::deliver_pending(ctx);
        }
        IpcMessage::ViewerClosing => {
            return true;
        }
        IpcMessage::RefreshNow { feed_id } => {
            let _ = ctx.refresh_tx.send(feed_id);
            let _ = out_tx.send(IpcMessage::Ack);
        }
        IpcMessage::SubscribeResolved { feed_url, title } => {
            let reply = subscribe(ctx, feed_url, title).await;
            let _ = out_tx.send(reply);
        }
        IpcMessage::ReloadConfig => {
            ctx.reload_config();
            let _ = out_tx.send(IpcMessage::Ack);
        }
        IpcMessage::ReadingState {
            feed_id,
            article_id,
            webkit,
        } => {
            // Fire-and-forget from the viewer; remember it for the next open.
            *ctx.reading_state.lock().expect("reading_state poisoned") =
                crate::state::ReadingState {
                    feed_id,
                    article_id,
                    webkit,
                };
        }
        IpcMessage::OpenViewer => {
            let _ = ctx.open_tx.send(OpenRequest::Show);
            let _ = out_tx.send(IpcMessage::Ack);
        }
        IpcMessage::OpenAt {
            feed_id,
            article_id,
        } => {
            let _ = ctx.open_tx.send(OpenRequest::At {
                feed_id,
                article_id,
            });
            let _ = out_tx.send(IpcMessage::Ack);
        }
        // Replies / daemon-outbound variants aren't expected inbound.
        other => {
            tracing::debug!("ignoring unexpected inbound message: {other:?}");
        }
    }
    false
}

/// Insert a resolved subscription, trigger an immediate poll, and tell the
/// viewer the feed set changed. Returns the reply to send back.
async fn subscribe(ctx: &AppCtx, feed_url: String, title: String) -> IpcMessage {
    let url = feed_url.clone();
    let result = ctx
        .with_conn(move |c| feeds::insert_feed(c, &url, &title))
        .await;

    match result {
        Ok(feed_id) => {
            tracing::info!("subscribed to {feed_url} (id {feed_id})");
            let _ = ctx.refresh_tx.send(Some(feed_id));
            ctx.send_to_viewer(IpcMessage::FeedsChanged);
            IpcMessage::Ack
        }
        Err(e) => {
            tracing::warn!("subscribe failed for {feed_url}: {e}");
            IpcMessage::Error(format!("subscribe failed: {e}"))
        }
    }
}
