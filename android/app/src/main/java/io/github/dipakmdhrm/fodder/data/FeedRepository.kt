package io.github.dipakmdhrm.fodder.data

import io.github.dipakmdhrm.fodder.data.db.ArticleDao
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity
import io.github.dipakmdhrm.fodder.data.db.FeedDao
import io.github.dipakmdhrm.fodder.data.db.FeedEntity
import io.github.dipakmdhrm.fodder.data.feed.DiscoveredFeed
import io.github.dipakmdhrm.fodder.data.feed.DiscoveryResult
import io.github.dipakmdhrm.fodder.data.feed.extractFeedLinks
import io.github.dipakmdhrm.fodder.data.feed.parseFeed
import io.github.dipakmdhrm.fodder.data.http.FetchResponse
import io.github.dipakmdhrm.fodder.data.http.backoffNext
import io.github.dipakmdhrm.fodder.data.http.conditionalGet
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import kotlin.time.Duration
import kotlin.time.Duration.Companion.minutes

/** What a single feed poll did, for the refresh summary. */
data class PollSummary(
    val newArticles: Int,
    val errors: Int,
    /** Ids of the genuinely-new rows, so the caller can notify about them. */
    val newArticleIds: List<Long> = emptyList(),
)

/** Base delay the exponential backoff grows from after a failed poll. */
private val BACKOFF_BASE = 5.minutes

/**
 * The poll-and-store workflow, kept out of the ViewModels and the worker so it
 * can be exercised directly.
 *
 * This is the Android counterpart of `fodderd/src/scheduler.rs`: same outcome
 * handling (Modified -> insert + clear error + store validators; NotModified /
 * RateLimited -> reschedule, keeping validators; Error -> record and back off),
 * minus the daemon, which Android has no equivalent of.
 */
class FeedRepository(
    private val feedDao: FeedDao,
    private val articleDao: ArticleDao,
    private val client: OkHttpClient,
    private val io: CoroutineDispatcher,
    private val now: () -> Long = { System.currentTimeMillis() },
    /** Spacing between scheduled polls of a healthy feed; follows the setting. */
    private val pollSpacing: () -> Duration = { 30.minutes },
) {
    fun observeFeeds() = feedDao.observeWithUnread()

    fun observeArticles(feedId: Long?) =
        if (feedId == null) articleDao.observeAll(limit = 500) else articleDao.observeByFeed(feedId)

    suspend fun feed(id: Long) = feedDao.byId(id)

    suspend fun article(id: Long) = articleDao.byId(id)

    suspend fun markRead(
        id: Long,
        read: Boolean = true,
    ) = articleDao.setRead(id, read)

    suspend fun markFeedRead(feedId: Long) = articleDao.markFeedRead(feedId)

    suspend fun rename(
        id: Long,
        title: String,
    ) = feedDao.rename(id, title)

    suspend fun delete(id: Long) = feedDao.delete(id)

    suspend fun unreadCount() = articleDao.unreadCount()

    /** Subscribe to an already-resolved feed URL, then poll it immediately. */
    suspend fun subscribe(
        url: String,
        title: String,
    ): Long {
        val id = feedDao.insert(FeedEntity(url = url, title = title, nextPollAt = now()))
        pollFeed(feedDao.byId(id) ?: return id)
        return id
    }

    /** Poll every feed whose backoff has elapsed. Used by the periodic worker. */
    suspend fun pollDue(): PollSummary = poll(feedDao.due(now()))

    /** Poll everything now. Used by pull-to-refresh. */
    suspend fun pollAll(): PollSummary = poll(feedDao.list())

    /** Poll one feed now. Used by the per-feed refresh action. */
    suspend fun pollOne(feedId: Long): PollSummary = poll(listOfNotNull(feedDao.byId(feedId)))

    private suspend fun poll(feeds: List<FeedEntity>): PollSummary {
        var newArticles = 0
        var errors = 0
        val ids = mutableListOf<Long>()
        for (feed in feeds) {
            val result = pollFeed(feed)
            newArticles += result.newArticles
            errors += result.errors
            ids += result.newArticleIds
        }
        return PollSummary(newArticles, errors, ids)
    }

    private suspend fun pollFeed(feed: FeedEntity): PollSummary {
        val response =
            withContext(io) {
                conditionalGet(client, feed.url, feed.etag, feed.lastModified)
            }

        return when (response) {
            is FetchResponse.NotModified -> {
                feedDao.reschedule(feed.id, now() + pollSpacing().inWholeMilliseconds)
                PollSummary(0, 0)
            }

            is FetchResponse.RateLimited -> {
                feedDao.reschedule(feed.id, now() + response.retryAfter.inWholeMilliseconds)
                PollSummary(0, 0)
            }

            is FetchResponse.Error -> {
                val delay = backoffNext(feed.errorCount, BACKOFF_BASE)
                feedDao.recordError(feed.id, response.message, now() + delay.inWholeMilliseconds)
                PollSummary(0, 1)
            }

            is FetchResponse.Modified -> {
                val parsed = withContext(io) { parseFeed(response.body) }
                if (parsed == null) {
                    val delay = backoffNext(feed.errorCount, BACKOFF_BASE)
                    feedDao.recordError(feed.id, "not a feed", now() + delay.inWholeMilliseconds)
                    return PollSummary(0, 1)
                }

                val rows =
                    parsed.items.map {
                        ArticleEntity(
                            feedId = feed.id,
                            guid = it.guid,
                            title = it.title,
                            url = it.url,
                            content = it.content,
                            publishedAt = it.publishedAt,
                            seenAt = now(),
                        )
                    }
                // -1 marks a row the unique (feedId, guid) index rejected, i.e.
                // an item we have already seen.
                val inserted = articleDao.insertAll(rows).filter { it != -1L }

                // Only fill the title from the feed document when the local one
                // is empty, so a title the user set is never clobbered.
                if (feed.title.isBlank() && !parsed.title.isNullOrBlank()) {
                    feedDao.rename(feed.id, parsed.title)
                }
                feedDao.recordSuccess(
                    id = feed.id,
                    etag = response.etag,
                    lastModified = response.lastModified,
                    nextPollAt = now() + pollSpacing().inWholeMilliseconds,
                )
                PollSummary(inserted.size, 0, inserted)
            }
        }
    }

    /**
     * Resolve a user-entered URL: a feed document is taken directly, otherwise
     * the page is scanned for `<link rel="alternate">` feeds.
     *
     * Port of `core/src/discovery.rs::resolve_feed`.
     */
    suspend fun resolve(url: String): DiscoveryResult =
        withContext(io) {
            when (val response = conditionalGet(client, url)) {
                is FetchResponse.Modified -> {
                    val feed = parseFeed(response.body)
                    if (feed != null) {
                        DiscoveryResult.Direct(url, feed.title)
                    } else {
                        val links =
                            extractFeedLinks(response.body.toString(Charsets.UTF_8), url)
                        if (links.isEmpty()) DiscoveryResult.None else DiscoveryResult.Candidates(links)
                    }
                }
                else -> DiscoveryResult.None
            }
        }

    /** Fetch a candidate's title so the subscribe sheet can show a real name. */
    suspend fun previewTitle(candidate: DiscoveredFeed): String =
        candidate.title
            ?: withContext(io) {
                val response = conditionalGet(client, candidate.url)
                (response as? FetchResponse.Modified)
                    ?.let { parseFeed(it.body)?.title }
                    ?: candidate.url
            }
}
