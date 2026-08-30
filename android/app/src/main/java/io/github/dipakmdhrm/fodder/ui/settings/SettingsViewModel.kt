package io.github.dipakmdhrm.fodder.ui.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.dipakmdhrm.fodder.data.FodderSettings
import io.github.dipakmdhrm.fodder.data.SettingsStore
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

class SettingsViewModel(private val store: SettingsStore) : ViewModel() {
    val settings: StateFlow<FodderSettings> =
        store.settings.stateIn(
            viewModelScope,
            SharingStarted.WhileSubscribed(5_000),
            FodderSettings(),
        )

    fun setPollInterval(minutes: Int) = viewModelScope.launch { store.setPollInterval(minutes) }

    fun setNotificationsEnabled(enabled: Boolean) = viewModelScope.launch { store.setNotificationsEnabled(enabled) }

    fun setOpenInWebViewByDefault(enabled: Boolean) = viewModelScope.launch { store.setOpenInWebViewByDefault(enabled) }
}
