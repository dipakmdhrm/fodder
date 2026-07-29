//! `PRAGMA user_version`-driven migration runner.
//!
//! Each entry in [`MIGRATIONS`] is a full SQL script applied in order. The
//! stored `user_version` records how many have run; on startup we apply only
//! the ones beyond it, each inside a transaction, then bump the version.

use rusqlite::Connection;

use super::DbError;

/// Ordered migration scripts. Append new ones; never edit or reorder existing
/// entries, as `user_version` indexes into this list.
pub const MIGRATIONS: &[&str] = &[
    // 0001 — initial schema.
    r#"
    CREATE TABLE feeds (
        id            INTEGER PRIMARY KEY,
        url           TEXT    NOT NULL UNIQUE,
        title         TEXT    NOT NULL DEFAULT '',
        etag          TEXT,
        last_modified TEXT,
        last_error    TEXT,
        error_count   INTEGER NOT NULL DEFAULT 0,
        next_poll_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
        created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
    );

    CREATE TABLE articles (
        id         INTEGER PRIMARY KEY,
        feed_id    INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
        guid       TEXT    NOT NULL,
        title      TEXT    NOT NULL DEFAULT '',
        url        TEXT,
        content    TEXT,
        published  TEXT,
        is_read    INTEGER NOT NULL DEFAULT 0,
        seen_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
        UNIQUE(feed_id, guid)
    );

    CREATE INDEX idx_articles_feed_unread ON articles(feed_id, is_read);
    CREATE INDEX idx_articles_published   ON articles(published DESC);
    "#,
];

/// Apply any migrations newer than the stored `user_version`.
pub fn run(conn: &Connection) -> Result<(), DbError> {
    let current: i64 =
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let current = current.max(0) as usize;

    for (idx, script) in MIGRATIONS.iter().enumerate().skip(current) {
        conn.execute_batch("BEGIN")?;
        if let Err(e) = conn.execute_batch(script) {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(DbError::Migration(format!(
                "migration {} failed: {e}",
                idx + 1
            )));
        }
        // user_version can't be parameterized; idx+1 is a trusted integer.
        conn.pragma_update(None, "user_version", (idx + 1) as i64)?;
        conn.execute_batch("COMMIT")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());

        // Running again applies nothing and does not error.
        run(&conn).unwrap();
        let tables: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('feeds','articles')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 2);
    }
}
