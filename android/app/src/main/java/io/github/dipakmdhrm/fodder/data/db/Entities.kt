package io.github.dipakmdhrm.fodder.data.db

import androidx.room.Entity
import androidx.room.ForeignKey
import androidx.room.Index
import androidx.room.PrimaryKey

/**
 * A subscribed feed and its conditional-GET / error bookkeeping.
 *
 * Mirrors `core/src/models.rs::Feed`. Timestamps are epoch millis rather than
 * the desktop's RFC 3339 text: the two stores are independent, and millis keep
 * the ordering queries trivial.
 */
@Entity(
    tableName = "feeds",
    indices = [Index(value = ["url"], unique = true)],
)
data class FeedEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    val url: String,
    val title: String,
    /** Last `ETag` seen, replayed as `If-None-Match`. */
    val etag: String? = null,
    /** Last `Last-Modified` seen, replayed as `If-Modified-Since`. */
    val lastModified: String? = null,
    /** Text of the most recent poll error, or null if healthy. */
    val lastError: String? = null,
    /** Consecutive error count; drives exponential backoff. */
    val errorCount: Int = 0,
    /** Earliest time this feed should be polled again. */
    val nextPollAt: Long = 0,
    val createdAt: Long = System.currentTimeMillis(),
)

/**
 * A stored article. [guid] is the stable dedupe key within a feed - the unique
 * index on (feedId, guid) is what makes re-polling idempotent, so a seen item
 * can never re-notify.
 *
 * Mirrors `core/src/models.rs::Article`.
 */
@Entity(
    tableName = "articles",
    foreignKeys = [
        ForeignKey(
            entity = FeedEntity::class,
            parentColumns = ["id"],
            childColumns = ["feedId"],
            onDelete = ForeignKey.CASCADE,
        ),
    ],
    indices = [
        Index(value = ["feedId", "guid"], unique = true),
        Index(value = ["feedId"]),
    ],
)
data class ArticleEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    val feedId: Long,
    val guid: String,
    val title: String,
    val url: String? = null,
    val content: String? = null,
    val publishedAt: Long? = null,
    val isRead: Boolean = false,
    val seenAt: Long = System.currentTimeMillis(),
)

/** A feed row plus its unread count, for the feed list. */
data class FeedWithUnread(
    val id: Long,
    val url: String,
    val title: String,
    val lastError: String?,
    val unreadCount: Int,
)

/**
 * A tombstone for an article the user deleted.
 *
 * Mirrors the `deleted_articles` table in `core/src/db/migrations.rs`. Deleting
 * a row from [ArticleEntity] is not enough on its own: dedupe is IGNORE against
 * the unique (feedId, guid) index, so an item still present in the feed
 * document would be re-inserted - and re-notified - by the next poll. Recording
 * the guid here lets the insert path skip it for good.
 *
 * The cascade means a feed's tombstones die with it, so unsubscribing and
 * resubscribing starts clean. [feedId] is the leftmost primary-key column,
 * which is what satisfies Room's index requirement for a foreign key.
 */
@Entity(
    tableName = "deleted_articles",
    primaryKeys = ["feedId", "guid"],
    foreignKeys = [
        ForeignKey(
            entity = FeedEntity::class,
            parentColumns = ["id"],
            childColumns = ["feedId"],
            onDelete = ForeignKey.CASCADE,
        ),
    ],
)
data class DeletedArticleEntity(
    val feedId: Long,
    val guid: String,
    val deletedAt: Long = System.currentTimeMillis(),
)
