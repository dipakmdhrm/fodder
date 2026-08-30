package io.github.dipakmdhrm.fodder.ui.articles

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
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
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.dipakmdhrm.fodder.appViewModelFactory
import io.github.dipakmdhrm.fodder.reader.htmlToPlainText
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
                        modifier =
                            Modifier.clickable {
                                viewModel.open(article.id)
                                onOpenArticle(article.id)
                            },
                    )
                    HorizontalDivider()
                }
            }
        }
    }
}
