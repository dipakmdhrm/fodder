package io.github.dipakmdhrm.fodder.ui.reader

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.dipakmdhrm.fodder.data.FeedRepository
import io.github.dipakmdhrm.fodder.data.SettingsStore
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity
import io.github.dipakmdhrm.fodder.reader.ReaderSpan
import io.github.dipakmdhrm.fodder.reader.htmlToSpans
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/** What the reader is showing: the sanitized text, or the live page. */
data class ReaderState(
    val article: ArticleEntity? = null,
    val spans: List<ReaderSpan> = emptyList(),
    val webView: Boolean = false,
)

class ReaderViewModel(
    private val repository: FeedRepository,
    private val settings: SettingsStore,
) : ViewModel() {
    private val _state = MutableStateFlow(ReaderState())
    val state: StateFlow<ReaderState> = _state.asStateFlow()

    fun bind(articleId: Long) {
        if (_state.value.article?.id == articleId) return
        viewModelScope.launch {
            val article = repository.article(articleId)
            val preferWeb = settings.settings.first().openInWebViewByDefault
            _state.value =
                ReaderState(
                    article = article,
                    spans = article?.content?.let { htmlToSpans(it) }.orEmpty(),
                    // Only honor the web-view preference when there is a URL to
                    // load; otherwise the light renderer is all we have.
                    webView = preferWeb && article?.url != null,
                )
            repository.markRead(articleId)
        }
    }

    fun toggleWebView() {
        val current = _state.value
        if (current.article?.url == null) return
        _state.value = current.copy(webView = !current.webView)
    }
}
