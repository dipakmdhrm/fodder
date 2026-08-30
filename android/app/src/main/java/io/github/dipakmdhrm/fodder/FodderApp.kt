package io.github.dipakmdhrm.fodder

import android.app.Application
import io.github.dipakmdhrm.fodder.data.FeedRepository
import io.github.dipakmdhrm.fodder.data.SettingsStore
import io.github.dipakmdhrm.fodder.data.db.FodderDatabase
import io.github.dipakmdhrm.fodder.work.Notifier
import io.github.dipakmdhrm.fodder.work.PollWorker
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit
import kotlin.time.Duration.Companion.minutes

/**
 * Manual dependency container - deliberately no Hilt/Koin at this size, matching
 * the sibling Android app's convention.
 */
class AppContainer(app: Application) {
    val database: FodderDatabase = FodderDatabase.open(app)

    val settings = SettingsStore(app)

    /** One shared client, like the desktop's single `reqwest::Client`. */
    private val client: OkHttpClient =
        OkHttpClient.Builder()
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .addInterceptor { chain ->
                chain.proceed(
                    chain.request().newBuilder()
                        .header("User-Agent", "FodderReader/${BuildConfig.VERSION_NAME}")
                        .build(),
                )
            }
            .build()

    @Volatile
    private var pollIntervalMinutes: Int = 60

    val repository =
        FeedRepository(
            feedDao = database.feedDao(),
            articleDao = database.articleDao(),
            client = client,
            io = Dispatchers.IO,
            pollSpacing = { pollIntervalMinutes.minutes },
        )

    @Volatile
    private var notifications: Boolean = true

    fun notificationsEnabled(): Boolean = notifications

    /** Keep the cached copies of the settings the poller reads in sync. */
    fun trackSettings(scope: CoroutineScope) {
        scope.launch {
            settings.settings.collect {
                pollIntervalMinutes = it.pollIntervalMinutes
                notifications = it.notificationsEnabled
            }
        }
    }
}

class FodderApp : Application() {
    lateinit var container: AppContainer
        private set

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onCreate() {
        super.onCreate()
        container = AppContainer(this)
        container.trackSettings(scope)
        Notifier(this).ensureChannel()

        scope.launch {
            val interval = container.settings.settings.first().pollIntervalMinutes
            PollWorker.schedule(this@FodderApp, interval)
        }
    }
}
