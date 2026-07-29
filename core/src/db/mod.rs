//! SQLite store: connection setup (WAL), migrations, and query modules.
//!
//! rusqlite is blocking; async callers (the viewer) wrap these in
//! `tokio::task::spawn_blocking`. The daemon is the primary writer. WAL mode +
//! a busy timeout let daemon and viewer share the file without corruption.

pub mod articles;
pub mod feeds;
pub mod migrations;

use std::path::Path;

use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use rusqlite::Connection;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration failed: {0}")]
    Migration(String),
}

/// Owned SQLite handle. Each process (and each blocking worker that needs
/// concurrent access) opens its own `Db`.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (creating if needed) the database at `path` and apply the pragmas
    /// required for safe shared access.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        Ok(Self { conn })
    }

    /// Open an in-memory database — used by tests.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        Ok(Self { conn })
    }

    fn configure(conn: &Connection) -> Result<(), DbError> {
        // WAL lets the daemon write while the viewer reads. NORMAL is durable
        // enough under WAL. busy_timeout retries instead of erroring on locks.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        Ok(())
    }

    /// Borrow the underlying connection for the query-module free functions.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Mutable borrow, needed for transactional bulk inserts.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Apply all pending migrations. Idempotent; safe to call on every startup.
    pub fn migrate(&mut self) -> Result<(), DbError> {
        migrations::run(&self.conn)
    }
}

// --- Timestamp helpers -------------------------------------------------------
//
// Timestamps are stored as RFC3339 TEXT in UTC (with a trailing `Z`). Column
// DEFAULTs use `strftime('%Y-%m-%dT%H:%M:%SZ','now')` to match, so even
// defaulted rows parse cleanly.

/// Format a UTC timestamp for storage.
pub fn to_sql_time(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Parse a stored timestamp. Accepts RFC3339 (our canonical form) and the
/// space-separated `YYYY-MM-DD HH:MM:SS` form SQLite's `datetime()` emits, so
/// we're tolerant of either. Falls back to the epoch on an unparseable value
/// rather than failing a whole query.
pub fn parse_sql_time(s: &str) -> DateTime<Utc> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return dt.with_timezone(&Utc);
    }
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Utc.from_utc_datetime(&naive);
    }
    Utc.timestamp_opt(0, 0).single().unwrap_or_default()
}
