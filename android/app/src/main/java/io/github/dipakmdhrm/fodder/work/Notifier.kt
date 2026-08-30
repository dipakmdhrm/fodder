package io.github.dipakmdhrm.fodder.work

import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.github.dipakmdhrm.fodder.MainActivity
import io.github.dipakmdhrm.fodder.R
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity

private const val CHANNEL_ID = "new_articles"

/**
 * Batched new-article notifications, one per feed.
 *
 * The desktop equivalent is `fodderd/src/notify.rs`, which posts one actionable
 * notification per feed and routes a click into the viewer. Here a tap deep-links
 * into the article via [MainActivity].
 */
class Notifier(private val context: Context) {
    fun ensureChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val channel =
            NotificationChannel(
                CHANNEL_ID,
                context.getString(R.string.channel_new_articles),
                NotificationManager.IMPORTANCE_DEFAULT,
            ).apply {
                description = context.getString(R.string.channel_new_articles_description)
            }
        context.getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    /**
     * Post one notification per feed that gained articles. Silently does nothing
     * when the user has not granted POST_NOTIFICATIONS - the refresh itself
     * still succeeded, so there is nothing to report as an error.
     */
    fun notifyNewArticles(
        feedTitle: String,
        articles: List<ArticleEntity>,
    ) {
        if (articles.isEmpty() || !canPost()) return
        ensureChannel()

        val first = articles.first()
        val title =
            if (articles.size == 1) {
                feedTitle
            } else {
                context.getString(R.string.notification_new_articles, articles.size, feedTitle)
            }
        val text = if (articles.size == 1) first.title else articles.joinToString("\n") { it.title }

        val intent =
            Intent(context, MainActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK
                putExtra(MainActivity.EXTRA_ARTICLE_ID, first.id)
            }
        val pending =
            PendingIntent.getActivity(
                context,
                first.feedId.toInt(),
                intent,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
            )

        val notification =
            NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_stat_fodder)
                .setContentTitle(title)
                .setContentText(if (articles.size == 1) first.title else articles.first().title)
                .setStyle(NotificationCompat.BigTextStyle().bigText(text))
                .setAutoCancel(true)
                .setContentIntent(pending)
                .build()

        // One notification per feed: the feed id keeps them from stacking up.
        NotificationManagerCompat.from(context).notify(first.feedId.toInt(), notification)
    }

    private fun canPost(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return true
        return ContextCompat.checkSelfPermission(
            context,
            android.Manifest.permission.POST_NOTIFICATIONS,
        ) == PackageManager.PERMISSION_GRANTED
    }
}
