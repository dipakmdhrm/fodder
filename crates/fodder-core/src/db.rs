use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::models::{Feed, Item, ItemSummary, NewItem};
use crate::{paths, Result};

pub struct Db {
    pub(crate) conn: Connection,
}

type Migration = fn(&Connection) -> rusqlite::Result<()>;

const MIGRATIONS: &[Migration] = &[migrate_v1];

fn migrate_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE feeds (
             id             INTEGER PRIMARY KEY,
             url            TEXT NOT NULL UNIQUE,
             title          TEXT NOT NULL DEFAULT '',
             site_url       TEXT,
             description    TEXT,
             etag           TEXT,
             last_modified  TEXT,
             last_polled_at INTEGER,
             last_status    TEXT,
             added_at       INTEGER NOT NULL
         );
         CREATE TABLE items (
             id           INTEGER PRIMARY KEY,
             feed_id      INTEGER NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
             guid         TEXT NOT NULL,
             title        TEXT NOT NULL DEFAULT '',
             link         TEXT,
             author       TEXT,
             published_at INTEGER,
             fetched_at   INTEGER NOT NULL,
             content_html TEXT,
             is_read      INTEGER NOT NULL DEFAULT 0,
             UNIQUE (feed_id, guid)
         );
         CREATE INDEX idx_items_feed_pub ON items(feed_id, published_at DESC);
         CREATE INDEX idx_items_unread ON items(is_read) WHERE is_read = 0;
         CREATE TABLE settings (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );",
    )
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&paths::db_path())
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        let version: usize =
            self.conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? as usize;
        for (i, migration) in MIGRATIONS.iter().enumerate().skip(version) {
            migration(&self.conn)?;
            self.conn
                .pragma_update(None, "user_version", (i + 1) as i64)?;
        }
        Ok(())
    }

    // ---- feeds ----

    pub fn add_feed(&self, url: &str, now: i64) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO feeds (url, added_at) VALUES (?1, ?2)",
            params![url, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn remove_feed(&self, feed_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM feeds WHERE id = ?1", params![feed_id])?;
        Ok(())
    }

    pub fn list_feeds(&self) -> Result<Vec<Feed>> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.url, f.title, f.site_url, f.description, f.etag,
                    f.last_modified, f.last_polled_at, f.last_status, f.added_at,
                    (SELECT COUNT(*) FROM items i WHERE i.feed_id = f.id AND i.is_read = 0)
             FROM feeds f
             ORDER BY f.title COLLATE NOCASE, f.id",
        )?;
        let feeds = stmt
            .query_map([], row_to_feed)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(feeds)
    }

    pub fn get_feed(&self, feed_id: i64) -> Result<Option<Feed>> {
        Ok(self
            .conn
            .query_row(
                "SELECT f.id, f.url, f.title, f.site_url, f.description, f.etag,
                        f.last_modified, f.last_polled_at, f.last_status, f.added_at,
                        (SELECT COUNT(*) FROM items i WHERE i.feed_id = f.id AND i.is_read = 0)
                 FROM feeds f WHERE f.id = ?1",
                params![feed_id],
                row_to_feed,
            )
            .optional()?)
    }

    pub fn update_feed_metadata(
        &self,
        feed_id: i64,
        title: &str,
        site_url: Option<&str>,
        description: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET title = ?2, site_url = ?3, description = ?4 WHERE id = ?1",
            params![feed_id, title, site_url, description],
        )?;
        Ok(())
    }

    /// Record the outcome of a poll attempt (both success and failure).
    pub fn record_poll(
        &self,
        feed_id: i64,
        etag: Option<&str>,
        last_modified: Option<&str>,
        status: &str,
        polled_at: i64,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET etag = ?2, last_modified = ?3, last_status = ?4,
                              last_polled_at = ?5
             WHERE id = ?1",
            params![feed_id, etag, last_modified, status, polled_at],
        )?;
        Ok(())
    }

    /// Update the subscription URL after a permanent (301/308) redirect.
    pub fn update_feed_url(&self, feed_id: i64, url: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE feeds SET url = ?2 WHERE id = ?1",
            params![feed_id, url],
        )?;
        Ok(())
    }

    // ---- items ----

    /// Insert items, ignoring ones already present (same feed_id + guid).
    /// Returns the number of genuinely new items.
    pub fn insert_items(
        &mut self,
        feed_id: i64,
        items: &[NewItem],
        fetched_at: i64,
    ) -> Result<u32> {
        let tx = self.conn.transaction()?;
        let mut new_count = 0u32;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO items
                     (feed_id, guid, title, link, author, published_at, fetched_at, content_html)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for item in items {
                new_count += stmt.execute(params![
                    feed_id,
                    item.guid,
                    item.title,
                    item.link,
                    item.author,
                    item.published_at,
                    fetched_at,
                    item.content_html,
                ])? as u32;
            }
        }
        tx.commit()?;
        Ok(new_count)
    }

    pub fn list_items(&self, feed_id: i64) -> Result<Vec<ItemSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, title, link, author, published_at, fetched_at, is_read
             FROM items WHERE feed_id = ?1
             ORDER BY COALESCE(published_at, fetched_at) DESC, id DESC",
        )?;
        let items = stmt
            .query_map(params![feed_id], |row| {
                Ok(ItemSummary {
                    id: row.get(0)?,
                    feed_id: row.get(1)?,
                    title: row.get(2)?,
                    link: row.get(3)?,
                    author: row.get(4)?,
                    published_at: row.get(5)?,
                    fetched_at: row.get(6)?,
                    is_read: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    pub fn get_item(&self, item_id: i64) -> Result<Option<Item>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, feed_id, guid, title, link, author, published_at,
                        fetched_at, content_html, is_read
                 FROM items WHERE id = ?1",
                params![item_id],
                |row| {
                    Ok(Item {
                        id: row.get(0)?,
                        feed_id: row.get(1)?,
                        guid: row.get(2)?,
                        title: row.get(3)?,
                        link: row.get(4)?,
                        author: row.get(5)?,
                        published_at: row.get(6)?,
                        fetched_at: row.get(7)?,
                        content_html: row.get(8)?,
                        is_read: row.get(9)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn set_item_read(&self, item_id: i64, read: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET is_read = ?2 WHERE id = ?1",
            params![item_id, read],
        )?;
        Ok(())
    }

    pub fn mark_feed_read(&self, feed_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE items SET is_read = 1 WHERE feed_id = ?1 AND is_read = 0",
            params![feed_id],
        )?;
        Ok(())
    }

    pub fn unread_total(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM items WHERE is_read = 0", [], |r| {
                r.get(0)
            })?)
    }
}

fn row_to_feed(row: &Row) -> rusqlite::Result<Feed> {
    Ok(Feed {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        site_url: row.get(3)?,
        description: row.get(4)?,
        etag: row.get(5)?,
        last_modified: row.get(6)?,
        last_polled_at: row.get(7)?,
        last_status: row.get(8)?,
        added_at: row.get(9)?,
        unread_count: row.get(10)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(guid: &str, title: &str) -> NewItem {
        NewItem {
            guid: guid.into(),
            title: title.into(),
            link: Some(format!("https://example.org/{guid}")),
            author: None,
            published_at: Some(1_700_000_000),
            content_html: Some("<p>hi</p>".into()),
        }
    }

    #[test]
    fn migrations_run_from_empty_and_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        drop(Db::open(&path).unwrap());
        // reopening must not re-run migrations
        let db = Db::open(&path).unwrap();
        assert_eq!(db.unread_total().unwrap(), 0);
    }

    #[test]
    fn insert_dedups_on_feed_and_guid() {
        let mut db = Db::open_in_memory().unwrap();
        let feed = db.add_feed("https://example.org/feed", 1).unwrap();
        let items = vec![item("a", "A"), item("b", "B")];
        assert_eq!(db.insert_items(feed, &items, 2).unwrap(), 2);
        // repoll with one old and one new item
        let items = vec![item("b", "B"), item("c", "C")];
        assert_eq!(db.insert_items(feed, &items, 3).unwrap(), 1);
        assert_eq!(db.list_items(feed).unwrap().len(), 3);
    }

    #[test]
    fn same_guid_in_different_feeds_is_not_a_dup() {
        let mut db = Db::open_in_memory().unwrap();
        let f1 = db.add_feed("https://one.example/feed", 1).unwrap();
        let f2 = db.add_feed("https://two.example/feed", 1).unwrap();
        assert_eq!(db.insert_items(f1, &[item("x", "X")], 2).unwrap(), 1);
        assert_eq!(db.insert_items(f2, &[item("x", "X")], 2).unwrap(), 1);
    }

    #[test]
    fn cascade_delete_removes_items() {
        let mut db = Db::open_in_memory().unwrap();
        let feed = db.add_feed("https://example.org/feed", 1).unwrap();
        db.insert_items(feed, &[item("a", "A")], 2).unwrap();
        db.remove_feed(feed).unwrap();
        assert_eq!(db.unread_total().unwrap(), 0);
    }

    #[test]
    fn read_state_and_unread_counts() {
        let mut db = Db::open_in_memory().unwrap();
        let feed = db.add_feed("https://example.org/feed", 1).unwrap();
        db.insert_items(feed, &[item("a", "A"), item("b", "B")], 2)
            .unwrap();
        assert_eq!(db.unread_total().unwrap(), 2);
        let first = db.list_items(feed).unwrap()[0].id;
        db.set_item_read(first, true).unwrap();
        assert_eq!(db.unread_total().unwrap(), 1);
        assert_eq!(db.list_feeds().unwrap()[0].unread_count, 1);
        db.mark_feed_read(feed).unwrap();
        assert_eq!(db.unread_total().unwrap(), 0);
    }

    #[test]
    fn poll_bookkeeping_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let feed = db.add_feed("https://example.org/feed", 1).unwrap();
        db.record_poll(feed, Some("\"etag\""), None, "ok", 42)
            .unwrap();
        let f = db.get_feed(feed).unwrap().unwrap();
        assert_eq!(f.etag.as_deref(), Some("\"etag\""));
        assert_eq!(f.last_polled_at, Some(42));
        assert_eq!(f.last_status.as_deref(), Some("ok"));
    }

    #[test]
    fn duplicate_subscription_url_rejected() {
        let db = Db::open_in_memory().unwrap();
        db.add_feed("https://example.org/feed", 1).unwrap();
        assert!(db.add_feed("https://example.org/feed", 2).is_err());
    }
}
