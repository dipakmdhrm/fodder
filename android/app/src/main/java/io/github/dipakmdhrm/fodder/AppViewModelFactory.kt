package io.github.dipakmdhrm.fodder

import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.CreationExtras
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import io.github.dipakmdhrm.fodder.ui.articles.ArticlesViewModel
import io.github.dipakmdhrm.fodder.ui.feeds.FeedsViewModel
import io.github.dipakmdhrm.fodder.ui.reader.ReaderViewModel
import io.github.dipakmdhrm.fodder.ui.settings.SettingsViewModel

/**
 * One factory for every ViewModel, reading the [AppContainer] off the
 * application rather than casting it at each call site.
 */
val appViewModelFactory =
    viewModelFactory {
        initializer { FeedsViewModel(container().repository) }
        initializer { ArticlesViewModel(container().repository) }
        initializer { ReaderViewModel(container().repository, container().settings) }
        initializer { SettingsViewModel(container().settings) }
    }

private fun CreationExtras.container(): AppContainer =
    (this[ViewModelProvider.AndroidViewModelFactory.APPLICATION_KEY] as FodderApp).container
