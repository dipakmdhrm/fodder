package io.github.dipakmdhrm.fodder.ui.articles

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.dipakmdhrm.fodder.appViewModelFactory
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity
import io.github.dipakmdhrm.fodder.reader.htmlToPlainText
import io.github.dipakmdhrm.fodder.ui.components.ConfirmDialog
import java.text.DateFormat
import java.util.Date

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ArticlesScreen(
    feedId: Long?,
    onOpenArticle: (Long) -> Unit,
    onBack: () -> Unit,
    viewModel: ArticlesViewModel = viewModel(factory = appViewModelFactory),
) {
    LaunchedEffect(feedId) { viewModel.bind(feedId) }

    val articles by viewModel.articles.collectAsStateWithLifecycle()
    val title by viewModel.title.collectAsStateWithLifecycle()
    var deleting by remember { mutableStateOf<ArticleEntity?>(null) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
            )
        },
    ) { padding ->
        if (articles.isEmpty()) {
            Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.Center) {
                Text("Nothing here yet", style = MaterialTheme.typography.bodyLarge)
            }
        } else {
            LazyColumn(Modifier.fillMaxSize().padding(padding)) {
                items(articles, key = { it.id }) { article ->
                    ArticleRow(
                        article = article,
                        onOpen = {
                            viewModel.open(article.id)
                            onOpenArticle(article.id)
                        },
                        onDelete = { deleting = article },
                    )
                    HorizontalDivider()
                }
            }
        }
    }

    deleting?.let { article ->
        ConfirmDialog(
            title = "Delete this item?",
            body = "This removes the article. It will not come back on the next refresh.",
            confirmLabel = "Delete",
            onConfirm = {
                viewModel.delete(article.id)
                deleting = null
            },
            onDismiss = { deleting = null },
        )
    }
}

@Composable
private fun ArticleRow(
    article: ArticleEntity,
    onOpen: () -> Unit,
    onDelete: () -> Unit,
) {
    var menuOpen by remember { mutableStateOf(false) }

    ListItem(
        headlineContent = {
            Text(
                article.title.ifBlank { "(untitled)" },
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                fontWeight = if (article.isRead) FontWeight.Normal else FontWeight.Bold,
            )
        },
        supportingContent = {
            val summary =
                article.content
                    ?.let { htmlToPlainText(it).trim().take(140) }
                    .orEmpty()
            val stamp =
                article.publishedAt?.let {
                    DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(it))
                }
            Text(
                listOfNotNull(stamp, summary.takeIf { it.isNotBlank() })
                    .joinToString(" - "),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        },
        trailingContent = {
            Box {
                IconButton(onClick = { menuOpen = true }) {
                    Icon(Icons.Default.MoreVert, contentDescription = "Article actions")
                }
                DropdownMenu(expanded = menuOpen, onDismissRequest = { menuOpen = false }) {
                    DropdownMenuItem(
                        text = { Text("Delete this item") },
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
