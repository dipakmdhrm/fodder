package io.github.dipakmdhrm.fodder.data.db

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.migration.Migration
import androidx.sqlite.db.SupportSQLiteDatabase

/**
 * The on-device store.
 *
 * The desktop app runs a hand-rolled `PRAGMA user_version` migration runner
 * (`core/src/db/migrations.rs`) because a daemon and a viewer share one file.
 * Here a single process owns the database, so Room's own migration machinery is
 * enough - bump [version] and add a Migration when the schema changes.
 */
@Database(
    entities = [FeedEntity::class, ArticleEntity::class, DeletedArticleEntity::class],
    version = 2,
    exportSchema = false,
)
abstract class FodderDatabase : RoomDatabase() {
    abstract fun feedDao(): FeedDao

    abstract fun articleDao(): ArticleDao

    companion object {
        /**
         * Adds the `deleted_articles` tombstone table, the counterpart of the
         * desktop's migration 0002.
         *
         * The DDL has to match what Room generates for [DeletedArticleEntity]
         * exactly - Room validates the schema on open and throws otherwise.
         * There is deliberately no `fallbackToDestructiveMigration()`: a
         * mismatch should fail loudly rather than quietly wipe someone's feeds.
         */
        val MIGRATION_1_2 =
            object : Migration(1, 2) {
                override fun migrate(db: SupportSQLiteDatabase) {
                    db.execSQL(
                        """
                        CREATE TABLE IF NOT EXISTS `deleted_articles` (
                            `feedId` INTEGER NOT NULL,
                            `guid` TEXT NOT NULL,
                            `deletedAt` INTEGER NOT NULL,
                            PRIMARY KEY(`feedId`, `guid`),
                            FOREIGN KEY(`feedId`) REFERENCES `feeds`(`id`)
                                ON UPDATE NO ACTION ON DELETE CASCADE
                        )
                        """.trimIndent(),
                    )
                }
            }

        fun open(context: Context): FodderDatabase =
            Room.databaseBuilder(
                context.applicationContext,
                FodderDatabase::class.java,
                "fodder.db",
            ).addMigrations(MIGRATION_1_2).build()
    }
}
