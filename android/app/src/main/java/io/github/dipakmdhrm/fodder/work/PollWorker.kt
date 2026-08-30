package io.github.dipakmdhrm.fodder.work

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import io.github.dipakmdhrm.fodder.FodderApp
import io.github.dipakmdhrm.fodder.data.FodderSettings
import java.util.concurrent.TimeUnit

/**
 * The periodic background refresh.
 *
 * This is as close as Android gets to `fodderd`'s poll loop, and the differences
 * are worth stating plainly: WorkManager will not run periodic work more often
 * than every 15 minutes, and Doze can delay a run well past its window. Delivery
 * is therefore best-effort - pull-to-refresh stays the way to force a poll.
 */
class PollWorker(
    context: Context,
    params: WorkerParameters,
) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        val container = (applicationContext as FodderApp).container
        return try {
            val summary = container.repository.pollDue()
            if (summary.newArticleIds.isNotEmpty() && container.notificationsEnabled()) {
                notify(container, summary.newArticleIds)
            }
            Result.success()
        } catch (e: Exception) {
            // A failed poll is normal (offline, DNS, a feed that is down). Let
            // WorkManager retry with its own backoff rather than giving up.
            Result.retry()
        }
    }

    private suspend fun notify(
        container: io.github.dipakmdhrm.fodder.AppContainer,
        newIds: List<Long>,
    ) {
        val articles = container.database.articleDao().byIds(newIds)
        val notifier = Notifier(applicationContext)
        // Batch per feed, matching the desktop's one-notification-per-feed rule.
        articles.groupBy { it.feedId }.forEach { (feedId, items) ->
            val title = container.repository.feed(feedId)?.title ?: return@forEach
            notifier.notifyNewArticles(title, items)
        }
    }

    companion object {
        private const val UNIQUE_NAME = "fodder-poll"

        /**
         * (Re)schedule the periodic poll. Called on startup and whenever the
         * interval setting changes; UPDATE keeps the existing work item rather
         * than restarting its window on every app launch.
         */
        fun schedule(
            context: Context,
            intervalMinutes: Int,
        ) {
            val interval =
                intervalMinutes.coerceAtLeast(FodderSettings.MIN_POLL_INTERVAL_MINUTES).toLong()
            val request =
                PeriodicWorkRequestBuilder<PollWorker>(interval, TimeUnit.MINUTES)
                    .setConstraints(
                        Constraints.Builder()
                            .setRequiredNetworkType(NetworkType.CONNECTED)
                            .build(),
                    )
                    .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(
                UNIQUE_NAME,
                ExistingPeriodicWorkPolicy.UPDATE,
                request,
            )
        }
    }
}
