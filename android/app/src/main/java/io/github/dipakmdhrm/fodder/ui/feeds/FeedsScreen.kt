package io.github.dipakmdhrm.fodder.ui.feeds

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.dipakmdhrm.fodder.appViewModelFactory
import io.github.dipakmdhrm.fodder.data.db.FeedWithUnread
import io.github.dipakmdhrm.fodder.ui.components.ConfirmDialog

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FeedsScreen(
    onOpenFeed: (Long) -> Unit,
    onOpenAllArticles: () -> Unit,
    onOpenSettings: () -> Unit,
    viewModel: FeedsViewModel = viewModel(factory = appViewModelFactory),
) {
    val feeds by viewModel.feeds.collectAsStateWithLifecycle()
    val refreshing by viewModel.refreshing.collectAsStateWithLifecycle()
    val message by viewModel.message.collectAsStateWithLifecycle()
    val addState by viewModel.addState.collectAsStateWithLifecycle()

    val snackbarHostState = remember { SnackbarHostState() }
    var showAddDialog by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf<FeedWithUnread?>(null) }
    var clearing by remember { mutableStateOf<FeedWithUnread?>(null) }
    var deleting by remember { mutableStateOf<FeedWithUnread?>(null) }

    LaunchedEffect(message) {
        message?.let {
            snackbarHostState.showSnackbar(it)
            viewModel.messageShown()
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text("Fodder") },
                actions = {
                    if (refreshing) {
                        CircularProgressIndicator(modifier = Modifier.padding(horizontal = 16.dp))
                    } else {
                        IconButton(onClick = viewModel::refreshAll) {
                            Icon(Icons.Default.Refresh, contentDescription = "Refresh all")
                        }
                    }
                    IconButton(onClick = onOpenSettings) {
                        Icon(Icons.Default.Settings, contentDescription = "Settings")
                    }
                },
            )
        },
        floatingActionButton = {
            FloatingActionButton(onClick = { showAddDialog = true }) {
                Icon(Icons.Default.Add, contentDescription = "Add feed")
            }
        },
    ) { padding ->
        if (feeds.isEmpty()) {
            EmptyState(Modifier.fillMaxSize().padding(padding))
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                item {
                    ListItem(
                        headlineContent = { Text("All articles") },
                        modifier = Modifier.clickable(onClick = onOpenAllArticles),
                    )
                    HorizontalDivider()
                }
                items(feeds, key = { it.id }) { feed ->
                    FeedRow(
                        feed = feed,
                        onOpen = { onOpenFeed(feed.id) },
                        onRefresh = { viewModel.refreshOne(feed.id) },
                        onMarkRead = { viewModel.markFeedRead(feed.id) },
                        onRename = { renaming = feed },
                        onClearItems = { clearing = feed },
                        onDelete = { deleting = feed },
                    )
                }
            }
        }
    }

    if (showAddDialog) {
        AddFeedDialog(
            state = addState,
            onResolve = viewModel::resolve,
            onPick = viewModel::subscribe,
            onDismiss = {
                showAddDialog = false
                viewModel.dismissAdd()
            },
        )
    }

    renaming?.let { feed ->
        RenameDialog(
            initial = feed.title,
            onConfirm = {
                viewModel.rename(feed.id, it)
                renaming = null
            },
            onDismiss = { renaming = null },
        )
    }

    clearing?.let { feed ->
        ConfirmDialog(
            title = "Delete all items?",
            body =
                "This deletes every stored article from this feed and keeps the " +
                    "subscription. They will not come back on the next refresh.",
            confirmLabel = "Delete all",
            onConfirm = {
                viewModel.clearItems(feed.id)
                clearing = null
            },
            onDismiss = { clearing = null },
        )
    }

    deleting?.let { feed ->
        ConfirmDialog(
            title = "Delete feed?",
            body = "This unsubscribes the feed and deletes its stored articles.",
            confirmLabel = "Delete",
            onConfirm = {
                viewModel.delete(feed.id)
                deleting = null
            },
            onDismiss = { deleting = null },
        )
    }
}

@Composable
private fun FeedRow(
    feed: FeedWithUnread,
    onOpen: () -> Unit,
    onRefresh: () -> Unit,
    onMarkRead: () -> Unit,
    onRename: () -> Unit,
    onClearItems: () -> Unit,
    onDelete: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }

    ListItem(
        headlineContent = {
            Text(
                text = feed.title.ifBlank { feed.url },
                // Unread feeds read as bold, matching the desktop sidebar.
                fontWeight = if (feed.unreadCount > 0) FontWeight.Bold else FontWeight.Normal,
            )
        },
        supportingContent = feed.lastError?.let { { Text(it, color = MaterialTheme.colorScheme.error) } },
        trailingContent = {
            Box {
                if (feed.unreadCount > 0) Badge { Text("${feed.unreadCount}") }
                IconButton(onClick = { menuOpen = true }) {
                    Icon(Icons.Default.MoreVert, contentDescription = "Feed actions")
                }
                DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                    DropdownMenuItem(
                        text = { Text("Refresh") },
                        onClick = {
                            menuOpen = false
                            onRefresh()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Mark all as read") },
                        onClick = {
                            menuOpen = false
                            onMarkRead()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Rename") },
                        onClick = {
                            menuOpen = false
                            onRename()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Delete all items") },
                        onClick = {
                            menuOpen = false
                            onClearItems()
                        },
                    )
                    DropdownMenuItem(
                        text = { Text("Delete") },
                        onClick = {
                            menuOpen = false
                            onDelete()
                        },
                    )
                }
            }
        },
        modifier = Modifier.clickable(onClick = onOpen),
    )
}

@Composable
private fun EmptyState(modifier: Modifier = Modifier) {
    Column(
        modifier = modifier,
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("No feeds yet", style = MaterialTheme.typography.titleMedium)
        Text(
            "Add a site address and Fodder will find its feed.",
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(top = 8.dp),
        )
    }
}

@Composable
private fun AddFeedDialog(
    state: AddFeedState,
    onResolve: (String) -> Unit,
    onPick: (io.github.dipakmdhrm.fodder.data.feed.DiscoveredFeed) -> Unit,
    onDismiss: () -> Unit,
) {
    var url by remember { mutableStateOf("") }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Add feed") },
        text = {
            Column {
                OutlinedTextField(
                    value = url,
                    onValueChange = { url = it },
                    label = { Text("Site or feed address") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                when (state) {
                    is AddFeedState.Resolving ->
                        CircularProgressIndicator(Modifier.padding(top = 16.dp))
                    is AddFeedState.Failed ->
                        Text(
                            state.message,
                            color = MaterialTheme.colorScheme.error,
                            modifier = Modifier.padding(top = 16.dp),
                        )
                    is AddFeedState.Choose ->
                        Column(Modifier.padding(top = 16.dp)) {
                            Text("Feeds found:", style = MaterialTheme.typography.labelLarge)
                            state.candidates.forEach { candidate ->
                                ListItem(
                                    headlineContent = { Text(candidate.title ?: candidate.url) },
                                    supportingContent = { Text(candidate.url) },
                                    modifier = Modifier.clickable { onPick(candidate) },
                                )
                            }
                        }
                    AddFeedState.Idle -> Unit
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onResolve(url.trim()) },
                enabled = url.isNotBlank() && state !is AddFeedState.Resolving,
            ) { Text("Find feed") }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun RenameDialog(
    initial: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var title by remember { mutableStateOf(initial) }

    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text("Rename feed") },
        text = {
            OutlinedTextField(
                value = title,
                onValueChange = { title = it },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(title.trim()) }, enabled = title.isNotBlank()) {
                Text("Rename")
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}
