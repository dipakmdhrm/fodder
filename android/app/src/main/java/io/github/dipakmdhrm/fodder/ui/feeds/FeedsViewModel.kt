package io.github.dipakmdhrm.fodder.ui.feeds

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.github.dipakmdhrm.fodder.data.FeedRepository
import io.github.dipakmdhrm.fodder.data.feed.DiscoveredFeed
import io.github.dipakmdhrm.fodder.data.feed.DiscoveryResult
import io.github.dipakmdhrm.fodder.work.formatRefreshSummary
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** What the add-feed sheet is currently showing. */
sealed interface AddFeedState {
    data object Idle : AddFeedState

    data object Resolving : AddFeedState

    data class Choose(val candidates: List<DiscoveredFeed>) : AddFeedState

    data class Failed(val message: String) : AddFeedState
}

class FeedsViewModel(private val repository: FeedRepository) : ViewModel() {
    val feeds =
        repository.observeFeeds()
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    private val _refreshing = MutableStateFlow(false)
    val refreshing: StateFlow<Boolean> = _refreshing.asStateFlow()

    /** One-shot message for the snackbar; cleared once shown. */
    private val _message = MutableStateFlow<String?>(null)
    val message: StateFlow<String?> = _message.asStateFlow()

    private val _addState = MutableStateFlow<AddFeedState>(AddFeedState.Idle)
    val addState: StateFlow<AddFeedState> = _addState.asStateFlow()

    fun refreshAll() {
        if (_refreshing.value) return
        _refreshing.value = true
        viewModelScope.launch {
            val started = System.currentTimeMillis()
            val summary = repository.pollAll()
            _message.value =
                formatRefreshSummary(
                    summary.newArticles,
                    summary.errors,
                    System.currentTimeMillis() - started,
                )
            _refreshing.value = false
        }
    }

    fun refreshOne(feedId: Long) {
        viewModelScope.launch {
            val started = System.currentTimeMillis()
            val summary = repository.pollOne(feedId)
            _message.value =
                formatRefreshSummary(
                    summary.newArticles,
                    summary.errors,
                    System.currentTimeMillis() - started,
                )
        }
    }

    fun markFeedRead(feedId: Long) = viewModelScope.launch { repository.markFeedRead(feedId) }

    fun rename(
        feedId: Long,
        title: String,
    ) = viewModelScope.launch { repository.rename(feedId, title) }

    fun delete(feedId: Long) = viewModelScope.launch { repository.delete(feedId) }

    /** Resolve a typed URL, subscribing straight away when it is itself a feed. */
    fun resolve(url: String) {
        _addState.value = AddFeedState.Resolving
        viewModelScope.launch {
            val normalized = if (url.startsWith("http")) url else "https://$url"
            when (val result = repository.resolve(normalized)) {
                is DiscoveryResult.Direct -> {
                    repository.subscribe(result.url, result.title.orEmpty())
                    _addState.value = AddFeedState.Idle
                }
                is DiscoveryResult.Candidates -> {
                    _addState.value = AddFeedState.Choose(result.feeds)
                }
                DiscoveryResult.None -> {
                    _addState.value = AddFeedState.Failed("No feed found at that address")
                }
            }
        }
    }

    fun subscribe(candidate: DiscoveredFeed) {
        viewModelScope.launch {
            repository.subscribe(candidate.url, repository.previewTitle(candidate))
            _addState.value = AddFeedState.Idle
        }
    }

    fun dismissAdd() {
        _addState.value = AddFeedState.Idle
    }

    fun messageShown() {
        _message.value = null
    }
}
