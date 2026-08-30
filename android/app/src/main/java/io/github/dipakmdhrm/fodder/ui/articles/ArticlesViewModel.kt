package io.github.dipakmdhrm.fodder.ui.articles

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.dipakmdhrm.fodder.data.FeedRepository
import io.github.dipakmdhrm.fodder.data.db.ArticleEntity
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

class ArticlesViewModel(private val repository: FeedRepository) : ViewModel() {
    private val feedId = MutableStateFlow<Long?>(null)

    private val _title = MutableStateFlow("All articles")
    val title: StateFlow<String> = _title.asStateFlow()

    @OptIn(ExperimentalCoroutinesApi::class)
    val articles: StateFlow<List<ArticleEntity>> =
        feedId
            .flatMapLatest { repository.observeArticles(it) }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    /** Called once from the screen; a null id means "all feeds". */
    fun bind(id: Long?) {
        if (feedId.value == id) return
        feedId.value = id
        viewModelScope.launch {
            _title.value = id?.let { repository.feed(it)?.title } ?: "All articles"
        }
    }

    /** Opening an article marks it read, matching the desktop's article list. */
    fun open(articleId: Long) = viewModelScope.launch { repository.markRead(articleId) }

    fun toggleRead(article: ArticleEntity) = viewModelScope.launch { repository.markRead(article.id, !article.isRead) }
}
