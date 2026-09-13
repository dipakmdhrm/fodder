package io.github.dipakmdhrm.fodder.data.db

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Transaction
import kotlinx.coroutines.flow.Flow

@Dao
interface FeedDao {
    @Insert(onConflict = OnConflictStrategy.ABORT)
    suspend fun insert(feed: FeedEntity): Long

    @Query("SELECT * FROM feeds ORDER BY title COLLATE NOCASE")
    suspend fun list(): List<FeedEntity>

    @Query("SELECT * FROM feeds WHERE id = :id")
    suspend fun byId(id: Long): FeedEntity?

    /** Feeds whose backoff has elapsed - the scheduled-poll working set. */
    @Query("SELECT * FROM feeds WHERE nextPollAt <= :now")
    suspend fun due(now: Long): List<FeedEntity>

    @Query("UPDATE feeds SET title = :title WHERE id = :id")
    suspend fun rename(
        id: Long,
        title: String,
    )

    @Query("DELETE FROM feeds WHERE id = :id")
    suspend fun delete(id: Long)

    /**
     * Record a successful poll: clear the error state and store the fresh
     * validators. Mirrors `core/src/db/feeds.rs::update_feed_success`.
     */
    @Query(
        """
        UPDATE feeds
           SET etag = :etag,
               lastModified = :lastModified,
               lastError = NULL,
               errorCount = 0,
               nextPollAt = :nextPollAt
         WHERE id = :id
        """,
    )
    suspend fun recordSuccess(
        id: Long,
        etag: String?,
        lastModified: String?,
        nextPollAt: Long,
    )

    /**
     * Record a failed poll. Deliberately leaves etag/lastModified alone so a
     * transient error does not throw away working validators.
     */
    @Query(
        """
        UPDATE feeds
           SET lastError = :error,
               errorCount = errorCount + 1,
               nextPollAt = :nextPollAt
         WHERE id = :id
        """,
    )
    suspend fun recordError(
        id: Long,
        error: String,
        nextPollAt: Long,
    )

    /**
     * Push back the next poll without touching validators or error state - the
     * 304 and rate-limited outcomes. Mirrors `core/src/db/feeds.rs::reschedule`.
     */
    @Query("UPDATE feeds SET nextPollAt = :nextPollAt WHERE id = :id")
    suspend fun reschedule(
        id: Long,
        nextPollAt: Long,
    )

    @Query(
        """
        SELECT f.id AS id, f.url AS url, f.title AS title, f.lastError AS lastError,
               (SELECT COUNT(*) FROM articles a WHERE a.feedId = f.id AND a.isRead = 0)
                   AS unreadCount
          FROM feeds f
         ORDER BY f.title COLLATE NOCASE
        """,
    )
    fun observeWithUnread(): Flow<List<FeedWithUnread>>
}

@Dao
interface ArticleDao {
    /**
     * Insert freshly-parsed items, ignoring ones already stored.
     *
     * The IGNORE conflict strategy against the unique (feedId, guid) index is
     * the Android equivalent of the desktop's `INSERT OR IGNORE`: the returned
     * list has -1 for every row that was already present, so callers can count
     * genuinely-new articles and never re-notify for a seen item.
     */
    @Insert(onConflict = OnConflictStrategy.IGNORE)
    suspend fun insertAll(articles: List<ArticleEntity>): List<Long>

    @Query("SELECT * FROM articles WHERE feedId = :feedId ORDER BY COALESCE(publishedAt, seenAt) DESC")
    fun observeByFeed(feedId: Long): Flow<List<ArticleEntity>>

    @Query("SELECT * FROM articles ORDER BY COALESCE(publishedAt, seenAt) DESC LIMIT :limit")
    fun observeAll(limit: Int): Flow<List<ArticleEntity>>

    @Query("SELECT * FROM articles WHERE id = :id")
    suspend fun byId(id: Long): ArticleEntity?

    @Query("SELECT COUNT(*) FROM articles WHERE isRead = 0")
    suspend fun unreadCount(): Int

    @Query("UPDATE articles SET isRead = :read WHERE id = :id")
    suspend fun setRead(
        id: Long,
        read: Boolean,
    )

    @Query("UPDATE articles SET isRead = 1 WHERE feedId = :feedId")
    suspend fun markFeedRead(feedId: Long)

    @Query("SELECT * FROM articles WHERE id IN (:ids)")
    suspend fun byIds(ids: List<Long>): List<ArticleEntity>

    /** Guids the user has deleted from this feed - see [DeletedArticleEntity]. */
    @Query("SELECT guid FROM deleted_articles WHERE feedId = :feedId")
    suspend fun deletedGuids(feedId: Long): List<String>

    /**
     * Insert freshly-parsed items, skipping both already-stored ones and ones
     * the user deleted.
     *
     * The Android counterpart of the desktop's tombstone-guarded
     * `INSERT OR IGNORE` (`core/src/db/articles.rs::insert_new_articles`). Room
     * cannot express the tombstone check inside `@Insert`, so the filter runs
     * here instead. The returned list keeps [insertAll]'s contract, so callers
     * still count genuinely-new rows and a deleted item can never re-notify.
     */
    @Transaction
    suspend fun insertNew(
        feedId: Long,
        articles: List<ArticleEntity>,
    ): List<Long> {
        val tombstoned = deletedGuids(feedId).toSet()
        return insertAll(articles.filterNot { it.guid in tombstoned })
    }

    @Query(
        """
        INSERT OR IGNORE INTO deleted_articles (feedId, guid, deletedAt)
        SELECT feedId, guid, :now FROM articles WHERE id = :id
        """,
    )
    suspend fun tombstoneArticle(
        id: Long,
        now: Long,
    )

    @Query("DELETE FROM articles WHERE id = :id")
    suspend fun deleteArticleRow(id: Long)

    /**
     * Delete one article and tombstone its guid, so the next poll of its feed
     * does not simply re-insert it. Mirrors
     * `core/src/db/articles.rs::delete_article`.
     */
    @Transaction
    suspend fun deleteArticle(
        id: Long,
        now: Long = System.currentTimeMillis(),
    ) {
        tombstoneArticle(id, now)
        deleteArticleRow(id)
    }

    @Query(
        """
        INSERT OR IGNORE INTO deleted_articles (feedId, guid, deletedAt)
        SELECT feedId, guid, :now FROM articles WHERE feedId = :feedId
        """,
    )
    suspend fun tombstoneFeedArticles(
        feedId: Long,
        now: Long,
    )

    @Query("DELETE FROM articles WHERE feedId = :feedId")
    suspend fun deleteArticlesForFeed(feedId: Long)

    /**
     * Delete every stored article for one feed, tombstoning each guid. The
     * subscription is kept - use [FeedDao.delete] to unsubscribe. Mirrors
     * `core/src/db/articles.rs::delete_articles_for`.
     */
    @Transaction
    suspend fun clearFeed(
        feedId: Long,
        now: Long = System.currentTimeMillis(),
    ) {
        tombstoneFeedArticles(feedId, now)
        deleteArticlesForFeed(feedId)
    }
}
