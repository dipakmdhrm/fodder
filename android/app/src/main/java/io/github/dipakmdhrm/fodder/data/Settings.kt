package io.github.dipakmdhrm.fodder.data

import android.content.Context
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.preferencesDataStore
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

private val Context.dataStore: DataStore<Preferences> by preferencesDataStore(name = "settings")

/** User preferences, the Android subset of `core/src/config.rs`. */
data class FodderSettings(
    val pollIntervalMinutes: Int = DEFAULT_POLL_INTERVAL_MINUTES,
    val notificationsEnabled: Boolean = true,
    val openInWebViewByDefault: Boolean = false,
) {
    companion object {
        /**
         * WorkManager refuses periodic work below 15 minutes, so unlike the
         * desktop (which clamps at 5) this is the floor Android can honor.
         */
        const val MIN_POLL_INTERVAL_MINUTES = 15
        const val DEFAULT_POLL_INTERVAL_MINUTES = 60

        /** The intervals offered in Settings, in minutes. */
        val CHOICES = listOf(15, 30, 60, 180, 360)
    }
}

/**
 * Reads and writes [FodderSettings]. Like the desktop's `Config::load`, a value
 * that is missing or out of range is normalized rather than rejected, so a bad
 * write can never leave the app unable to poll.
 */
class SettingsStore(private val context: Context) {
    val settings: Flow<FodderSettings> =
        context.dataStore.data.map { prefs ->
            FodderSettings(
                pollIntervalMinutes =
                    (prefs[POLL_INTERVAL] ?: FodderSettings.DEFAULT_POLL_INTERVAL_MINUTES)
                        .coerceAtLeast(FodderSettings.MIN_POLL_INTERVAL_MINUTES),
                notificationsEnabled = prefs[NOTIFICATIONS] ?: true,
                openInWebViewByDefault = prefs[WEB_VIEW_DEFAULT] ?: false,
            )
        }

    suspend fun setPollInterval(minutes: Int) {
        context.dataStore.edit {
            it[POLL_INTERVAL] = minutes.coerceAtLeast(FodderSettings.MIN_POLL_INTERVAL_MINUTES)
        }
    }

    suspend fun setNotificationsEnabled(enabled: Boolean) {
        context.dataStore.edit { it[NOTIFICATIONS] = enabled }
    }

    suspend fun setOpenInWebViewByDefault(enabled: Boolean) {
        context.dataStore.edit { it[WEB_VIEW_DEFAULT] = enabled }
    }

    private companion object {
        val POLL_INTERVAL = intPreferencesKey("poll_interval_minutes")
        val NOTIFICATIONS = booleanPreferencesKey("notifications_enabled")
        val WEB_VIEW_DEFAULT = booleanPreferencesKey("open_in_webview_by_default")
    }
}
