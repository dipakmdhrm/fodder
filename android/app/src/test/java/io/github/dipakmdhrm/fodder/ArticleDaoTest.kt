package io.github.dipakmdhrm.fodder

import android.app.Application
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity
import io.github.dipakmdhrm.fodder.data.db.FeedEntity
import io.github.dipakmdhrm.fodder.data.db.FodderDatabase
import kotlinx.coroutines.test.runTest
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Mirrors `core/src/db/articles.rs`'s tests: the dedupe, cascade, and unread
 * behavior the poller and the UI both depend on.
 *
 * Robolectric supplies the Android runtime Room needs, so this stays a JVM test
 * and CI needs no emulator.
 */
@RunWith(RobolectricTestRunner::class)
// A plain Application, not FodderApp: booting the real one would schedule the
// WorkManager poll, which has no place in a database test.
@Config(sdk = [34], application = Application::class)
class ArticleDaoTest {
    private lateinit var db: FodderDatabase

    @Before
    fun setUp() {
        db =
            Room.inMemoryDatabaseBuilder(
                ApplicationProvider.getApplicationContext(),
                FodderDatabase::class.java,
            ).allowMainThreadQueries().build()
    }

    @After
    fun tearDown() = db.close()

    private suspend fun seedFeed(url: String = "https://e.com/feed") =
        db.feedDao().insert(FeedEntity(url = url, title = "Example"))

    private fun article(
        feedId: Long,
        guid: String,
        read: Boolean = false,
    ) = ArticleEntity(feedId = feedId, guid = guid, title = "T-$guid", isRead = read)

    @Test
    fun `re-inserting a seen guid returns no new ids`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.articleDao()

            val first = dao.insertAll(listOf(article(feedId, "g1"), article(feedId, "g2")))
            assertEquals(2, first.count { it != -1L })

            // The same items on the next poll: nothing new, so nothing re-notifies.
            val second = dao.insertAll(listOf(article(feedId, "g1"), article(feedId, "g2")))
            assertEquals(0, second.count { it != -1L })

            // A genuinely new item still comes through.
            val third = dao.insertAll(listOf(article(feedId, "g2"), article(feedId, "g3")))
            assertEquals(1, third.count { it != -1L })
        }

    @Test
    fun `the same guid in two feeds is two articles`() =
        runTest {
            val a = seedFeed("https://a.com/feed")
            val b = seedFeed("https://b.com/feed")
            val dao = db.articleDao()

            assertEquals(1, dao.insertAll(listOf(article(a, "shared"))).count { it != -1L })
            assertEquals(1, dao.insertAll(listOf(article(b, "shared"))).count { it != -1L })
        }

    @Test
    fun `deleting a feed cascades to its articles`() =
        runTest {
            val feedId = seedFeed()
            db.articleDao().insertAll(listOf(article(feedId, "g1"), article(feedId, "g2")))
            assertEquals(2, db.articleDao().unreadCount())

            db.feedDao().delete(feedId)
            assertEquals(0, db.articleDao().unreadCount())
        }

    @Test
    fun `unread counts follow mark-read and back again`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.articleDao()
            val ids = dao.insertAll(listOf(article(feedId, "g1"), article(feedId, "g2")))
            assertEquals(2, dao.unreadCount())

            dao.setRead(ids[0], true)
            assertEquals(1, dao.unreadCount())

            dao.setRead(ids[0], false)
            assertEquals(2, dao.unreadCount())
        }

    @Test
    fun `mark all as read clears one feed only`() =
        runTest {
            val a = seedFeed("https://a.com/feed")
            val b = seedFeed("https://b.com/feed")
            val dao = db.articleDao()
            dao.insertAll(listOf(article(a, "g1"), article(a, "g2"), article(b, "g3")))

            dao.markFeedRead(a)
            assertEquals(1, dao.unreadCount())
        }

    @Test
    fun `a successful poll clears the error and stores validators`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.feedDao()

            dao.recordError(feedId, "boom", nextPollAt = 100)
            dao.byId(feedId)!!.let {
                assertEquals("boom", it.lastError)
                assertEquals(1, it.errorCount)
            }

            dao.recordSuccess(feedId, etag = "\"v1\"", lastModified = "then", nextPollAt = 200)
            dao.byId(feedId)!!.let {
                assertEquals(null, it.lastError)
                assertEquals(0, it.errorCount)
                assertEquals("\"v1\"", it.etag)
            }
        }

    @Test
    fun `rescheduling preserves the stored validators`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.feedDao()
            dao.recordSuccess(feedId, etag = "\"v1\"", lastModified = "then", nextPollAt = 100)

            // A 304 or a rate limit must not throw away working validators.
            dao.reschedule(feedId, nextPollAt = 999)
            dao.byId(feedId)!!.let {
                assertEquals("\"v1\"", it.etag)
                assertEquals("then", it.lastModified)
                assertEquals(999, it.nextPollAt)
            }
        }

    @Test
    fun `due filters on the next poll time`() =
        runTest {
            val soon = seedFeed("https://soon.com/feed")
            val later = seedFeed("https://later.com/feed")
            val dao = db.feedDao()
            dao.reschedule(soon, nextPollAt = 100)
            dao.reschedule(later, nextPollAt = 10_000)

            assertEquals(listOf(soon), dao.due(now = 500).map { it.id })
        }

    @Test
    fun `a deleted article is not re-inserted by the next poll`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.articleDao()
            val rows = listOf(article(feedId, "g1"), article(feedId, "g2"))
            val ids = dao.insertNew(feedId, rows)

            dao.deleteArticle(ids.first { it != -1L })
            assertEquals(1, dao.unreadCount())

            // The whole point: the feed document still carries the item, and it
            // must stay gone - and must not count as new.
            val again = dao.insertNew(feedId, rows)
            assertEquals(0, again.count { it != -1L })
            assertEquals(1, dao.unreadCount())
        }

    @Test
    fun `clearing a feed empties it and blocks re-insert`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.articleDao()
            val rows = listOf(article(feedId, "g1"), article(feedId, "g2"))
            dao.insertNew(feedId, rows)

            dao.clearFeed(feedId)
            assertEquals(0, dao.unreadCount())
            // The subscription survives; only its articles are gone.
            assertNotNull(db.feedDao().byId(feedId))

            val again = dao.insertNew(feedId, rows)
            assertEquals(0, again.count { it != -1L })
        }

    @Test
    fun `a tombstone only applies to its own feed`() =
        runTest {
            val feedId = seedFeed()
            val other = seedFeed("https://e.com/other")
            val dao = db.articleDao()
            dao.insertNew(feedId, listOf(article(feedId, "g1")))

            dao.clearFeed(feedId)

            val ids = dao.insertNew(other, listOf(article(other, "g1")))
            assertEquals(1, ids.count { it != -1L })
        }

    @Test
    fun `deleting a feed clears its tombstones`() =
        runTest {
            val feedId = seedFeed()
            val dao = db.articleDao()
            dao.insertNew(feedId, listOf(article(feedId, "g1")))
            dao.clearFeed(feedId)
            assertEquals(listOf("g1"), dao.deletedGuids(feedId))

            db.feedDao().delete(feedId)
            assertEquals(emptyList<String>(), dao.deletedGuids(feedId))
        }
}
