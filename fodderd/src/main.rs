//! fodderd — the headless Fodder Reader daemon.
//!
//! Owns the poll loop, the shared SQLite database, desktop notifications, and
//! the single-instance IPC socket. The system-tray icon and on-demand viewer
//! spawning land in M3; for now a notification click or a viewer connection
//! drives the open-request path.

mod notify;
mod scheduler;
mod server;
mod single_instance;
mod state;

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use fodder_core::db::Db;
use fodder_core::ipc::IpcMessage;
use fodder_core::poller::Poller;
use fodder_core::{paths, Config};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

use single_instance::Acquired;
use state::{AppCtx, OpenRequest};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let socket_path = paths::daemon_socket_path()?;

    // Enforce a single daemon. A second launch just asks the running one to
    // open the viewer, then exits.
    let listener = match single_instance::acquire(&socket_path).await? {
        Acquired::Primary(listener) => listener,
        Acquired::AlreadyRunning => {
            tracing::info!("daemon already running; requesting it open the viewer");
            single_instance::request_open(&socket_path).await?;
            return Ok(());
        }
    };
    tracing::info!("fodderd {} listening on {}", env!("CARGO_PKG_VERSION"), socket_path.display());

    // Load config and open the shared database.
    let config = Config::load(&paths::config_path()?).context("loading config")?;
    let mut db = Db::open(&paths::db_path()?).context("opening database")?;
    db.migrate().context("running migrations")?;

    let poller = Poller::new(config.poll_concurrency);

    let (open_tx, open_rx) = mpsc::unbounded_channel::<OpenRequest>();
    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<Option<i64>>();

    let ctx = AppCtx {
        db: Arc::new(Mutex::new(db)),
        poller: Arc::new(poller),
        config: Arc::new(config),
        viewer: Arc::new(Mutex::new(None)),
        open_tx,
        refresh_tx,
    };

    // Long-running tasks.
    let server_ctx = ctx.clone();
    let server_task = tokio::spawn(async move { server::run(listener, server_ctx).await });

    let sched_ctx = ctx.clone();
    let sched_task = tokio::spawn(async move { scheduler::run(sched_ctx, refresh_rx).await });

    let open_ctx = ctx.clone();
    let open_task = tokio::spawn(async move { run_open_handler(open_ctx, open_rx).await });

    // Run until interrupted, then clean up the socket file.
    wait_for_shutdown().await;
    tracing::info!("shutting down");

    server_task.abort();
    sched_task.abort();
    open_task.abort();
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

/// Route open requests to the viewer if one is connected. Spawning a viewer when
/// none exists lands in M3; until then we log the intent.
async fn run_open_handler(ctx: AppCtx, mut open_rx: UnboundedReceiver<OpenRequest>) {
    while let Some(req) = open_rx.recv().await {
        let delivered = match &req {
            OpenRequest::Show => ctx.send_to_viewer(IpcMessage::OpenViewer),
            OpenRequest::At {
                feed_id,
                article_id,
            } => ctx.send_to_viewer(IpcMessage::OpenAt {
                feed_id: *feed_id,
                article_id: *article_id,
            }),
        };
        if !delivered {
            tracing::info!(
                "open requested ({req:?}) but no viewer connected; spawn lands in M3"
            );
        }
    }
}

/// Resolve when the process receives SIGINT or SIGTERM.
async fn wait_for_shutdown() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
    }
}
