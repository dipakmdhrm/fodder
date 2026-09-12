package io.github.dipakmdhrm.fodder

import android.app.Application
import android.database.sqlite.SQLiteDatabase
import androidx.room.Room
import androidx.test.core.app.ApplicationProvider
import io.github.dipakmdhrm.fodder.data.db.FodderDatabase
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Pins the version 1 -> 2 upgrade, the Android counterpart of the desktop's
 * "migrations apply and are idempotent" test.
 *
 * The in-memory database the other tests use runs Room's `createAllTables`, so
 * it never exercises a migration. Here the v1 schema is built by hand - exactly
 * what shipped before tombstones - and Room is then opened at v2. Opening is
 * itself the assertion: Room compares the migrated schema against what it
 * expects and throws if the hand-written DDL in `MIGRATION_1_2` drifts from
 * `DeletedArticleEntity`.
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [34], application = Application::class)
class MigrationTest {
    /** Room's generated `createAllTables` for schema version 1, verbatim. */
    private val v1Schema =
        listOf(
            "CREATE TABLE IF NOT EXISTS `feeds` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, " +
                "`url` TEXT NOT NULL, `title` TEXT NOT NULL, `etag` TEXT, `lastModified` TEXT, " +
                "`lastError` TEXT, `errorCount` INTEGER NOT NULL, `nextPollAt` INTEGER NOT NULL, " +
                "`createdAt` INTEGER NOT NULL)",
            "CREATE UNIQUE INDEX IF NOT EXISTS `index_feeds_url` ON `feeds` (`url`)",
            "CREATE TABLE IF NOT EXISTS `articles` (`id` INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL, " +
                "`feedId` INTEGER NOT NULL, `guid` TEXT NOT NULL, `title` TEXT NOT NULL, `url` TEXT, " +
                "`content` TEXT, `publishedAt` INTEGER, `isRead` INTEGER NOT NULL, " +
                "`seenAt` INTEGER NOT NULL, FOREIGN KEY(`feedId`) REFERENCES `feeds`(`id`) " +
                "ON UPDATE NO ACTION ON DELETE CASCADE )",
            "CREATE UNIQUE INDEX IF NOT EXISTS `index_articles_feedId_guid` ON `articles` (`feedId`, `guid`)",
            "CREATE INDEX IF NOT EXISTS `index_articles_feedId` ON `articles` (`feedId`)",
        )

    @Test
    fun `upgrading from v1 adds tombstones and keeps existing data`() =
        runTest {
            val context = ApplicationProvider.getApplicationContext<Application>()
            val name = "migration-v1.db"
            context.deleteDatabase(name)

            val file = context.getDatabasePath(name)
            file.parentFile?.mkdirs()
            SQLiteDatabase.openOrCreateDatabase(file, null).use { raw ->
                v1Schema.forEach(raw::execSQL)
                raw.execSQL(
                    "INSERT INTO feeds (url, title, errorCount, nextPollAt, createdAt) " +
                        "VALUES ('https://e.com/feed', 'Example', 0, 0, 0)",
                )
                raw.execSQL(
                    "INSERT INTO articles (feedId, guid, title, isRead, seenAt) " +
                        "VALUES (1, 'g1', 'T-g1', 0, 0)",
                )
                raw.version = 1
            }

            val db =
                Room.databaseBuilder(context, FodderDatabase::class.java, name)
                    .addMigrations(FodderDatabase.MIGRATION_1_2)
                    .allowMainThreadQueries()
                    .build()

            try {
                // The subscription and its article survived the upgrade.
                assertEquals(1, db.feedDao().list().size)
                assertEquals(1, db.articleDao().unreadCount())

                // And the new table is usable, cascade included.
                db.articleDao().clearFeed(1)
                assertEquals(listOf("g1"), db.articleDao().deletedGuids(1))
                assertEquals(0, db.articleDao().unreadCount())
            } finally {
                db.close()
                context.deleteDatabase(name)
            }
        }
}
