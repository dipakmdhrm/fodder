//! fodderd — the headless Fodder Reader daemon.
//!
//! Owns the poll loop, the shared SQLite database, desktop notifications, and
//! the single-instance IPC socket. The system-tray icon and on-demand viewer
//! spawning land in M3; for now a notification click or a viewer connection
//! drives the open-request path.

mod notify;
mod reminder;
mod scheduler;
mod server;
mod single_instance;
mod state;
mod tray;
mod viewer_proc;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use fodder_core::db::Db;
use fodder_core::poller::Poller;
use fodder_core::{paths, Config};
use tokio::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::Notify;

use single_instance::Acquired;
use state::{AppCtx, OpenRequest};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // `--open-viewer` (used by the app-menu launcher) opens the viewer once the
    // daemon is up. Autostart launches `fodderd` without it, staying headless.
    let open_viewer = std::env::args().any(|arg| arg == "--open-viewer");

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
    tracing::info!(
        "fodderd {} listening on {}",
        env!("CARGO_PKG_VERSION"),
        socket_path.display()
    );

    // Load config and open the shared database.
    let config = Config::load(&paths::config_path()?).context("loading config")?;
    let mut db = Db::open(&paths::db_path()?).context("opening database")?;
    db.migrate().context("running migrations")?;

    let poller = Poller::new(config.poll_concurrency);

    let (open_tx, open_rx) = mpsc::unbounded_channel::<OpenRequest>();
    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel::<Option<i64>>();
    let shutdown = Arc::new(Notify::new());

    let ctx = AppCtx {
        db: Arc::new(Mutex::new(db)),
        poller: Arc::new(poller),
        config: Arc::new(RwLock::new(config)),
        reminder_reload: Arc::new(Notify::new()),
        viewer: Arc::new(Mutex::new(None)),
        viewer_alive: Arc::new(AtomicBool::new(false)),
        pending_open: Arc::new(Mutex::new(None)),
        viewer_kill: Arc::new(Mutex::new(None)),
        open_tx: open_tx.clone(),
        refresh_tx: refresh_tx.clone(),
        reading_state: Arc::new(Mutex::new(state::ReadingState::default())),
    };

    // Best-effort system tray; graceful degrade if no SNI host is present.
    let tray_handle = tray::try_spawn(open_tx, refresh_tx, shutdown.clone()).await;

    // Long-running tasks.
    let server_ctx = ctx.clone();
    let server_task = tokio::spawn(async move { server::run(listener, server_ctx).await });

    let sched_ctx = ctx.clone();
    let sched_task = tokio::spawn(async move { scheduler::run(sched_ctx, refresh_rx).await });

    let open_ctx = ctx.clone();
    let open_task = tokio::spawn(async move { run_open_handler(open_ctx, open_rx).await });

    let reminder_ctx = ctx.clone();
    let reminder_task = tokio::spawn(async move { reminder::run(reminder_ctx).await });

    // Opened from the app menu → spawn the viewer now that the daemon is up.
    if open_viewer {
        let _ = ctx.open_tx.send(OpenRequest::Show);
    }

    // Run until interrupted or the tray's Quit is chosen.
    wait_for_shutdown(&shutdown).await;
    tracing::info!("shutting down");

    // Terminate the viewer child, tear down the tray, and clean up the socket.
    viewer_proc::kill(&ctx);
    if let Some(handle) = tray_handle {
        handle.shutdown().await;
    }
    server_task.abort();
    sched_task.abort();
    open_task.abort();
    reminder_task.abort();
    let _ = std::fs::remove_file(&socket_path);

    Ok(())
}

/// Route open requests: raise/navigate a running viewer, or spawn a new one.
async fn run_open_handler(ctx: AppCtx, mut open_rx: UnboundedReceiver<OpenRequest>) {
    while let Some(req) = open_rx.recv().await {
        viewer_proc::open(&ctx, req);
    }
}

/// Resolve when the process receives SIGINT/SIGTERM or the tray requests quit.
async fn wait_for_shutdown(shutdown: &Notify) {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => {}
        _ = sigterm.recv() => {}
        _ = shutdown.notified() => {}
    }
}
