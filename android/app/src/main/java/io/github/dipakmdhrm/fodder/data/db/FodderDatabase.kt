package io.github.dipakmdhrm.fodder.data.db

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase

/**
 * The on-device store.
 *
 * The desktop app runs a hand-rolled `PRAGMA user_version` migration runner
 * (`core/src/db/migrations.rs`) because a daemon and a viewer share one file.
 * Here a single process owns the database, so Room's own migration machinery is
 * enough - bump [version] and add a Migration when the schema changes.
 */
@Database(
    entities = [FeedEntity::class, ArticleEntity::class],
    version = 1,
    exportSchema = false,
)
abstract class FodderDatabase : RoomDatabase() {
    abstract fun feedDao(): FeedDao

    abstract fun articleDao(): ArticleDao

    companion object {
        fun open(context: Context): FodderDatabase =
            Room.databaseBuilder(
                context.applicationContext,
                FodderDatabase::class.java,
                "fodder.db",
            ).build()
    }
}
