//! Feed CRUD and conditional-GET / error bookkeeping queries.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{parse_sql_time, to_sql_time, DbError};
use crate::models::Feed;

fn row_to_feed(row: &Row) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: row.get("id")?,
        url: row.get("url")?,
        title: row.get("title")?,
        etag: row.get("etag")?,
        last_modified: row.get("last_modified")?,
        last_error: row.get("last_error")?,
        error_count: row.get("error_count")?,
        next_poll_at: parse_sql_time(&row.get::<_, String>("next_poll_at")?),
        created_at: parse_sql_time(&row.get::<_, String>("created_at")?),
    })
}

const SELECT: &str = "SELECT id, url, title, etag, last_modified, last_error, \
    error_count, next_poll_at, created_at FROM feeds";

/// Insert a new feed, returning its row id. Errors if the URL already exists
/// (the `UNIQUE` constraint). New feeds are due immediately (default
/// `next_poll_at`).
pub fn insert_feed(conn: &Connection, url: &str, title: &str) -> Result<i64, DbError> {
    conn.execute(
        "INSERT INTO feeds (url, title) VALUES (?1, ?2)",
        params![url, title],
    )?;
    Ok(conn.last_insert_rowid())
}

/// All feeds, ordered by title then url for a stable UI list.
pub fn list_feeds(conn: &Connection) -> Result<Vec<Feed>, DbError> {
    let sql = format!("{SELECT} ORDER BY title COLLATE NOCASE, url");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_feed)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One feed by id, or `None`.
pub fn get_feed(conn: &Connection, id: i64) -> Result<Option<Feed>, DbError> {
    let sql = format!("{SELECT} WHERE id = ?1");
    Ok(conn.query_row(&sql, params![id], row_to_feed).optional()?)
}

/// Feeds whose `next_poll_at` is at or before `now` — the poll scheduler's work
/// list, soonest first.
pub fn feeds_due(conn: &Connection, now: DateTime<Utc>) -> Result<Vec<Feed>, DbError> {
    let sql = format!("{SELECT} WHERE next_poll_at <= ?1 ORDER BY next_poll_at");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![to_sql_time(now)], row_to_feed)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Record a successful poll: store fresh validators, clear the error state, and
/// schedule the next poll.
pub fn update_feed_success(
    conn: &Connection,
    id: i64,
    etag: Option<&str>,
    last_modified: Option<&str>,
    next_poll_at: DateTime<Utc>,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE feeds SET etag = ?2, last_modified = ?3, last_error = NULL, \
         error_count = 0, next_poll_at = ?4 WHERE id = ?1",
        params![id, etag, last_modified, to_sql_time(next_poll_at)],
    )?;
    Ok(())
}

/// Reschedule a feed after a poll that produced no new body — a 304 Not
/// Modified or a rate-limit backoff. Clears the error state and sets the next
/// poll time but deliberately preserves `etag` / `last_modified`, so the stored
/// validators keep working on the next conditional GET.
pub fn reschedule(conn: &Connection, id: i64, next_poll_at: DateTime<Utc>) -> Result<(), DbError> {
    conn.execute(
        "UPDATE feeds SET last_error = NULL, error_count = 0, next_poll_at = ?2 WHERE id = ?1",
        params![id, to_sql_time(next_poll_at)],
    )?;
    Ok(())
}

/// Update the feed title (from a parsed feed or a rename).
pub fn update_feed_title(conn: &Connection, id: i64, title: &str) -> Result<(), DbError> {
    conn.execute(
        "UPDATE feeds SET title = ?2 WHERE id = ?1",
        params![id, title],
    )?;
    Ok(())
}

/// Record a failed poll: set the error text, bump `error_count`, and schedule a
/// backed-off retry. The caller computes `error_count`/`next_poll_at`.
pub fn update_feed_error(
    conn: &Connection,
    id: i64,
    error: &str,
    error_count: i32,
    next_poll_at: DateTime<Utc>,
) -> Result<(), DbError> {
    conn.execute(
        "UPDATE feeds SET last_error = ?2, error_count = ?3, next_poll_at = ?4 WHERE id = ?1",
        params![id, error, error_count, to_sql_time(next_poll_at)],
    )?;
    Ok(())
}

/// Delete a feed. Its articles cascade away via the FK (`foreign_keys=ON`).
pub fn delete_feed(conn: &Connection, id: i64) -> Result<(), DbError> {
    conn.execute("DELETE FROM feeds WHERE id = ?1", params![id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn setup() -> Db {
        let mut db = Db::open_in_memory().unwrap();
        db.migrate().unwrap();
        db
    }

    #[test]
    fn insert_get_list_roundtrip() {
        let db = setup();
        let id = insert_feed(db.conn(), "https://example.com/feed.xml", "Example").unwrap();
        let got = get_feed(db.conn(), id).unwrap().unwrap();
        assert_eq!(got.url, "https://example.com/feed.xml");
        assert_eq!(got.title, "Example");
        assert_eq!(got.error_count, 0);
        assert_eq!(list_feeds(db.conn()).unwrap().len(), 1);
    }

    #[test]
    fn duplicate_url_rejected() {
        let db = setup();
        insert_feed(db.conn(), "https://example.com/feed.xml", "A").unwrap();
        let err = insert_feed(db.conn(), "https://example.com/feed.xml", "B");
        assert!(err.is_err());
    }

    #[test]
    fn success_clears_error_and_stores_validators() {
        let db = setup();
        let id = insert_feed(db.conn(), "https://e.com/f", "E").unwrap();
        let next = Utc::now() + chrono::Duration::minutes(30);
        update_feed_error(db.conn(), id, "boom", 2, Utc::now()).unwrap();
        update_feed_success(
            db.conn(),
            id,
            Some("etag-1"),
            Some("Mon, 01 Jan 2024"),
            next,
        )
        .unwrap();
        let f = get_feed(db.conn(), id).unwrap().unwrap();
        assert_eq!(f.etag.as_deref(), Some("etag-1"));
        assert_eq!(f.last_error, None);
        assert_eq!(f.error_count, 0);
    }

    #[test]
    fn reschedule_preserves_validators() {
        let db = setup();
        let id = insert_feed(db.conn(), "https://e.com/f", "E").unwrap();
        update_feed_success(db.conn(), id, Some("etag-x"), Some("lm-x"), Utc::now()).unwrap();
        // A 304 reschedule must keep the validators so the next GET stays conditional.
        reschedule(db.conn(), id, Utc::now() + chrono::Duration::minutes(30)).unwrap();
        let f = get_feed(db.conn(), id).unwrap().unwrap();
        assert_eq!(f.etag.as_deref(), Some("etag-x"));
        assert_eq!(f.last_modified.as_deref(), Some("lm-x"));
        assert_eq!(f.last_error, None);
    }

    #[test]
    fn feeds_due_filters_by_time() {
        let db = setup();
        let id = insert_feed(db.conn(), "https://e.com/f", "E").unwrap();
        // Push next_poll_at into the future; nothing should be due now.
        update_feed_success(
            db.conn(),
            id,
            None,
            None,
            Utc::now() + chrono::Duration::hours(1),
        )
        .unwrap();
        assert!(feeds_due(db.conn(), Utc::now()).unwrap().is_empty());
        // A time past the schedule returns it.
        let due = feeds_due(db.conn(), Utc::now() + chrono::Duration::hours(2)).unwrap();
        assert_eq!(due.len(), 1);
    }
}
