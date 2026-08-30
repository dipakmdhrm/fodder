package io.github.dipakmdhrm.fodder.ui.settings

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.dipakmdhrm.fodder.BuildConfig
import io.github.dipakmdhrm.fodder.appViewModelFactory
import io.github.dipakmdhrm.fodder.data.FodderSettings
import io.github.dipakmdhrm.fodder.work.PollWorker

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(
    onBack: () -> Unit,
    viewModel: SettingsViewModel = viewModel(factory = appViewModelFactory),
) {
    val settings by viewModel.settings.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var pickingInterval by remember { mutableStateOf(false) }

    // Any interval change has to reach WorkManager, or the setting would only
    // take effect on the next cold start.
    LaunchedEffect(settings.pollIntervalMinutes) {
        PollWorker.schedule(context, settings.pollIntervalMinutes)
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Settings") },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            ListItem(
                headlineContent = { Text("Refresh every") },
                supportingContent = { Text(formatInterval(settings.pollIntervalMinutes)) },
                modifier = Modifier.clickable { pickingInterval = true },
            )
            HorizontalDivider()
            ListItem(
                headlineContent = { Text("New-article notifications") },
                trailingContent = {
                    Switch(
                        checked = settings.notificationsEnabled,
                        onCheckedChange = viewModel::setNotificationsEnabled,
                    )
                },
            )
            HorizontalDivider()
            ListItem(
                headlineContent = { Text("Open the full article by default") },
                supportingContent = { Text("Load the live page instead of the reader view") },
                trailingContent = {
                    Switch(
                        checked = settings.openInWebViewByDefault,
                        onCheckedChange = viewModel::setOpenInWebViewByDefault,
                    )
                },
            )
            HorizontalDivider()
            Text(
                "Android refreshes in the background at best effort: the system " +
                    "will not run the check more often than every 15 minutes, and " +
                    "may delay it further to save battery. Pull to refresh for an " +
                    "immediate update.",
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(16.dp),
            )
            Text(
                "Fodder ${BuildConfig.VERSION_NAME}",
                style = MaterialTheme.typography.labelSmall,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
    }

    if (pickingInterval) {
        AlertDialog(
            onDismissRequest = { pickingInterval = false },
            title = { Text("Refresh every") },
            text = {
                Column {
                    FodderSettings.CHOICES.forEach { minutes ->
                        ListItem(
                            headlineContent = { Text(formatInterval(minutes)) },
                            modifier =
                                Modifier.clickable {
                                    viewModel.setPollInterval(minutes)
                                    pickingInterval = false
                                },
                        )
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = { pickingInterval = false }) { Text("Cancel") }
            },
        )
    }
}

private fun formatInterval(minutes: Int): String =
    when {
        minutes < 60 -> "$minutes minutes"
        minutes == 60 -> "1 hour"
        else -> "${minutes / 60} hours"
    }
