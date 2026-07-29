//! Article queries: dedupe insert, read-state, unread counts, listing.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::{parse_sql_time, to_sql_time, DbError};
use crate::models::{Article, NewArticle};

fn row_to_article(row: &Row) -> rusqlite::Result<Article> {
    Ok(Article {
        id: row.get("id")?,
        feed_id: row.get("feed_id")?,
        guid: row.get("guid")?,
        title: row.get("title")?,
        url: row.get("url")?,
        content: row.get("content")?,
        published: row
            .get::<_, Option<String>>("published")?
            .as_deref()
            .map(parse_sql_time),
        is_read: row.get::<_, i64>("is_read")? != 0,
        seen_at: parse_sql_time(&row.get::<_, String>("seen_at")?),
    })
}

const SELECT: &str = "SELECT id, feed_id, guid, title, url, content, published, \
    is_read, seen_at FROM articles";

/// Insert new articles for `feed_id`, deduping on `(feed_id, guid)`. Returns the
/// row ids that were *actually inserted* (i.e. genuinely new) so the caller can
/// notify only those. Already-seen items are silently ignored, so they never
/// re-notify — even across daemon restarts. Runs in a single transaction.
pub fn insert_new_articles(
    conn: &mut Connection,
    feed_id: i64,
    items: &[NewArticle],
) -> Result<Vec<i64>, DbError> {
    let tx = conn.transaction()?;
    let mut new_ids = Vec::new();
    {
        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO articles \
             (feed_id, guid, title, url, content, published) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for item in items {
            let changed = stmt.execute(params![
                feed_id,
                item.guid,
                item.title,
                item.url,
                item.content,
                item.published.map(to_sql_time),
            ])?;
            // execute() returns rows changed: 1 = inserted, 0 = ignored (dupe).
            if changed == 1 {
                new_ids.push(tx.last_insert_rowid());
            }
        }
    }
    tx.commit()?;
    Ok(new_ids)
}

/// Mark a single article read.
pub fn mark_read(conn: &Connection, article_id: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE articles SET is_read = 1 WHERE id = ?1",
        params![article_id],
    )?;
    Ok(())
}

/// Mark a single article unread.
pub fn mark_unread(conn: &Connection, article_id: i64) -> Result<(), DbError> {
    conn.execute(
        "UPDATE articles SET is_read = 0 WHERE id = ?1",
        params![article_id],
    )?;
    Ok(())
}

/// Mark all articles read, optionally scoped to one feed (`None` = every feed).
pub fn mark_all_read(conn: &Connection, feed_id: Option<i64>) -> Result<(), DbError> {
    match feed_id {
        Some(id) => conn.execute(
            "UPDATE articles SET is_read = 1 WHERE feed_id = ?1 AND is_read = 0",
            params![id],
        )?,
        None => conn.execute("UPDATE articles SET is_read = 1 WHERE is_read = 0", [])?,
    };
    Ok(())
}

/// Unread count per feed id. Feeds with no unread articles are omitted.
pub fn unread_counts(conn: &Connection) -> Result<HashMap<i64, i64>, DbError> {
    let mut stmt =
        conn.prepare("SELECT feed_id, COUNT(*) FROM articles WHERE is_read = 0 GROUP BY feed_id")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    let mut map = HashMap::new();
    for r in rows {
        let (feed_id, count) = r?;
        map.insert(feed_id, count);
    }
    Ok(map)
}

/// Total unread across all feeds — for the tray / window title.
pub fn total_unread(conn: &Connection) -> Result<i64, DbError> {
    Ok(
        conn.query_row("SELECT COUNT(*) FROM articles WHERE is_read = 0", [], |r| {
            r.get(0)
        })?,
    )
}

/// Articles for one feed, or the "All articles" aggregate when `feed_id` is
/// `None`. Newest first (by published, falling back to seen time).
pub fn articles_for(
    conn: &Connection,
    feed_id: Option<i64>,
    limit: i64,
) -> Result<Vec<Article>, DbError> {
    let order = " ORDER BY COALESCE(published, seen_at) DESC LIMIT ?";
    match feed_id {
        Some(id) => {
            let sql = format!("{SELECT} WHERE feed_id = ?1{order}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![id, limit], row_to_article)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        }
        None => {
            let sql = format!("{SELECT}{order}");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![limit], row_to_article)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }
}

/// One article by id, or `None`.
pub fn get_article(conn: &Connection, id: i64) -> Result<Option<Article>, DbError> {
    let sql = format!("{SELECT} WHERE id = ?1");
    Ok(conn
        .query_row(&sql, params![id], row_to_article)
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{feeds, Db};

    fn new_article(guid: &str, title: &str) -> NewArticle {
        NewArticle {
            guid: guid.to_string(),
            title: title.to_string(),
            url: Some(format!("https://e.com/{guid}")),
            content: Some("body".into()),
            published: None,
        }
    }

    fn setup() -> (Db, i64) {
        let mut db = Db::open_in_memory().unwrap();
        db.migrate().unwrap();
        let feed_id = feeds::insert_feed(db.conn(), "https://e.com/f", "E").unwrap();
        (db, feed_id)
    }

    #[test]
    fn insert_or_ignore_dedupes() {
        let (mut db, feed_id) = setup();
        let items = vec![new_article("a", "A"), new_article("b", "B")];
        let first = insert_new_articles(db.conn_mut(), feed_id, &items).unwrap();
        assert_eq!(first.len(), 2, "both are new on first insert");

        // Re-inserting the same guids yields no new ids -> no re-notify.
        let second = insert_new_articles(db.conn_mut(), feed_id, &items).unwrap();
        assert!(second.is_empty(), "already-seen items are ignored");

        // A mix: one old, one new.
        let mixed = vec![new_article("b", "B"), new_article("c", "C")];
        let third = insert_new_articles(db.conn_mut(), feed_id, &mixed).unwrap();
        assert_eq!(third.len(), 1, "only the genuinely new item returns an id");
    }

    #[test]
    fn cascade_delete_removes_articles() {
        let (mut db, feed_id) = setup();
        insert_new_articles(db.conn_mut(), feed_id, &[new_article("a", "A")]).unwrap();
        assert_eq!(total_unread(db.conn()).unwrap(), 1);
        feeds::delete_feed(db.conn(), feed_id).unwrap();
        assert_eq!(
            total_unread(db.conn()).unwrap(),
            0,
            "articles cascade-deleted"
        );
    }

    #[test]
    fn unread_counts_and_mark_read() {
        let (mut db, feed_id) = setup();
        let ids = insert_new_articles(
            db.conn_mut(),
            feed_id,
            &[new_article("a", "A"), new_article("b", "B")],
        )
        .unwrap();
        assert_eq!(*unread_counts(db.conn()).unwrap().get(&feed_id).unwrap(), 2);

        mark_read(db.conn(), ids[0]).unwrap();
        assert_eq!(*unread_counts(db.conn()).unwrap().get(&feed_id).unwrap(), 1);

        mark_all_read(db.conn(), Some(feed_id)).unwrap();
        assert!(!unread_counts(db.conn()).unwrap().contains_key(&feed_id));
        assert_eq!(total_unread(db.conn()).unwrap(), 0);
    }

    #[test]
    fn mark_read_then_unread_toggles() {
        let (mut db, feed_id) = setup();
        let ids = insert_new_articles(db.conn_mut(), feed_id, &[new_article("a", "A")]).unwrap();
        mark_read(db.conn(), ids[0]).unwrap();
        assert_eq!(total_unread(db.conn()).unwrap(), 0);
        mark_unread(db.conn(), ids[0]).unwrap();
        assert_eq!(total_unread(db.conn()).unwrap(), 1);
    }

    #[test]
    fn articles_for_all_and_single() {
        let (mut db, feed_id) = setup();
        insert_new_articles(
            db.conn_mut(),
            feed_id,
            &[new_article("a", "A"), new_article("b", "B")],
        )
        .unwrap();
        assert_eq!(
            articles_for(db.conn(), Some(feed_id), 100).unwrap().len(),
            2
        );
        assert_eq!(articles_for(db.conn(), None, 100).unwrap().len(), 2);
    }
}
