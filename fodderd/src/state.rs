//! Shared daemon state and the blocking-DB bridge.

use std::sync::{Arc, Mutex};

use fodder_core::db::{Db, DbError};
use fodder_core::ipc::IpcMessage;
use fodder_core::poller::Poller;
use fodder_core::Config;
use rusqlite::Connection;
use tokio::sync::mpsc::UnboundedSender;

/// A request to bring the viewer to the foreground (from a notification click,
/// a second `fodderd` launch, or a tray action in M3).
#[derive(Debug, Clone)]
pub enum OpenRequest {
    /// Just show the window.
    Show,
    /// Show the window and navigate to a feed / article.
    At {
        feed_id: i64,
        article_id: Option<i64>,
    },
}

/// The shared SQLite handle. rusqlite is blocking and `Connection` is `Send`
/// but not `Sync`, so we guard it with a `Mutex` and touch it only from
/// `spawn_blocking` closures.
pub type DbHandle = Arc<Mutex<Db>>;

/// Cloneable handle to everything the daemon's tasks share.
#[derive(Clone)]
pub struct AppCtx {
    pub db: DbHandle,
    pub poller: Arc<Poller>,
    pub config: Arc<Config>,
    /// Outbound channel to the currently-connected viewer, if any. Set on
    /// `ViewerHello`, cleared on disconnect.
    pub viewer: Arc<Mutex<Option<UnboundedSender<IpcMessage>>>>,
    /// Requests to open/raise the viewer.
    pub open_tx: UnboundedSender<OpenRequest>,
    /// Requests to poll now: `Some(feed_id)` for one feed, `None` for all due.
    pub refresh_tx: UnboundedSender<Option<i64>>,
}

impl AppCtx {
    /// Run a closure against the database on the blocking pool. The closure gets
    /// a `&mut Connection`; read-only queries taking `&Connection` still work
    /// via reborrow.
    pub async fn with_conn<F, T>(&self, f: F) -> anyhow::Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
        T: Send + 'static,
    {
        let db = self.db.clone();
        let out = tokio::task::spawn_blocking(move || {
            let mut guard = db.lock().expect("db mutex poisoned");
            f(guard.conn_mut())
        })
        .await?;
        Ok(out?)
    }

    /// Send a message to the viewer if one is connected. Returns `true` if it
    /// was delivered to the outbound queue.
    pub fn send_to_viewer(&self, msg: IpcMessage) -> bool {
        if let Some(tx) = self.viewer.lock().expect("viewer mutex poisoned").as_ref() {
            tx.send(msg).is_ok()
        } else {
            false
        }
    }
}
