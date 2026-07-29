//! The tokio↔glib async bridge.
//!
//! GTK is single-threaded: widgets may only be touched on the main thread. The
//! blocking SQLite work and async IPC run on a tokio runtime; results cross back
//! to the GTK main context via `glib::spawn_future_local`, whose future runs on
//! the main thread so its continuation can safely mutate widgets.

use std::sync::{Arc, Mutex};

use fodder_core::db::{Db, DbError};
use gtk4::glib;
use rusqlite::Connection;

/// Shared, blocking database handle. Guarded by a `Mutex` and only touched
/// inside `spawn_blocking` closures.
pub type DbHandle = Arc<Mutex<Db>>;

/// Run a blocking DB query on the tokio blocking pool, then invoke `then` on the
/// GTK main thread with the result. `then` may safely touch widgets.
pub fn run_db<T, F, G>(handle: &tokio::runtime::Handle, db: DbHandle, query: F, then: G)
where
    T: Send + 'static,
    F: FnOnce(&mut Connection) -> Result<T, DbError> + Send + 'static,
    G: FnOnce(anyhow::Result<T>) + 'static,
{
    let handle = handle.clone();
    glib::spawn_future_local(async move {
        let joined = handle
            .spawn_blocking(move || {
                let mut guard = db.lock().expect("db mutex poisoned");
                query(guard.conn_mut())
            })
            .await;
        let result = match joined {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(e)) => Err(anyhow::Error::new(e)),
            Err(e) => Err(anyhow::anyhow!("db task failed: {e}")),
        };
        then(result);
    });
}
