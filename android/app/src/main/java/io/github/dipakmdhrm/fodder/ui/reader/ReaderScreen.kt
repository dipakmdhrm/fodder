package io.github.dipakmdhrm.fodder.ui.reader

import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Article
import androidx.compose.material.icons.filled.Public
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import io.github.dipakmdhrm.fodder.appViewModelFactory
import io.github.dipakmdhrm.fodder.reader.ReaderSpan

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ReaderScreen(
    articleId: Long,
    onBack: () -> Unit,
    viewModel: ReaderViewModel = viewModel(factory = appViewModelFactory),
) {
    LaunchedEffect(articleId) { viewModel.bind(articleId) }
    val state by viewModel.state.collectAsStateWithLifecycle()

    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        state.article?.title.orEmpty(),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                    }
                },
                actions = {
                    if (state.article?.url != null) {
                        IconButton(onClick = viewModel::toggleWebView) {
                            if (state.webView) {
                                Icon(Icons.Default.Article, contentDescription = "Reader view")
                            } else {
                                Icon(Icons.Default.Public, contentDescription = "Full article")
                            }
                        }
                    }
                },
            )
        },
    ) { padding ->
        val url = state.article?.url
        if (state.webView && url != null) {
            FullArticleView(url, Modifier.fillMaxSize().padding(padding))
        } else {
            Column(
                Modifier
                    .fillMaxSize()
                    .padding(padding)
                    .padding(16.dp)
                    .verticalScroll(rememberScrollState()),
            ) {
                Text(state.spans.toAnnotated(), style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

private fun List<ReaderSpan>.toAnnotated() =
    buildAnnotatedString {
        this@toAnnotated.forEach { span ->
            withStyle(
                SpanStyle(
                    fontWeight = if (span.bold) FontWeight.Bold else null,
                    fontStyle = if (span.italic) FontStyle.Italic else null,
                    fontFamily = if (span.monospace) FontFamily.Monospace else null,
                    fontSize = if (span.heading) 20.sp else androidx.compose.ui.unit.TextUnit.Unspecified,
                ),
            ) {
                append(span.text)
            }
        }
    }

/**
 * The full-article view.
 *
 * Mirrors the desktop WebKit toggle's posture: load the live URL with
 * JavaScript off and no storage, so the page renders but cannot run scripts or
 * persist anything.
 */
@Composable
private fun FullArticleView(
    url: String,
    modifier: Modifier = Modifier,
) {
    AndroidView(
        modifier = modifier,
        factory = { context ->
            WebView(context).apply {
                webViewClient = WebViewClient()
                settings.javaScriptEnabled = false
                settings.domStorageEnabled = false
                settings.cacheMode = WebSettings.LOAD_NO_CACHE
                loadUrl(url)
            }
        },
        update = { it.loadUrl(url) },
    )
}
