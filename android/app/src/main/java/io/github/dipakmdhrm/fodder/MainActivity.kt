package io.github.dipakmdhrm.fodder

import android.Manifest
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.LaunchedEffect
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import io.github.dipakmdhrm.fodder.ui.articles.ArticlesScreen
import io.github.dipakmdhrm.fodder.ui.feeds.FeedsScreen
import io.github.dipakmdhrm.fodder.ui.nav.Routes
import io.github.dipakmdhrm.fodder.ui.reader.ReaderScreen
import io.github.dipakmdhrm.fodder.ui.settings.SettingsScreen
import io.github.dipakmdhrm.fodder.ui.theme.FodderTheme

class MainActivity : ComponentActivity() {
    private val requestNotifications =
        registerForActivityResult(ActivityResultContracts.RequestPermission()) { }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            requestNotifications.launch(Manifest.permission.POST_NOTIFICATIONS)
        }

        // A notification tap carries the article to open.
        val deepLinkArticleId = intent?.getLongExtra(EXTRA_ARTICLE_ID, -1L)?.takeIf { it > 0 }

        setContent {
            FodderTheme {
                Surface(color = MaterialTheme.colorScheme.background) {
                    val navController = rememberNavController()

                    LaunchedEffect(deepLinkArticleId) {
                        deepLinkArticleId?.let { navController.navigate(Routes.reader(it)) }
                    }

                    NavHost(navController = navController, startDestination = Routes.FEEDS) {
                        composable(Routes.FEEDS) {
                            FeedsScreen(
                                onOpenFeed = { navController.navigate(Routes.articles(it)) },
                                onOpenAllArticles = {
                                    navController.navigate(Routes.articles(Routes.ALL_FEEDS))
                                },
                                onOpenSettings = { navController.navigate(Routes.SETTINGS) },
                            )
                        }
                        composable(
                            Routes.ARTICLES,
                            arguments =
                                listOf(
                                    navArgument(Routes.ARTICLES_ARG_FEED_ID) {
                                        type = NavType.LongType
                                    },
                                ),
                        ) { entry ->
                            val feedId =
                                entry.arguments?.getLong(Routes.ARTICLES_ARG_FEED_ID)
                                    ?: Routes.ALL_FEEDS
                            ArticlesScreen(
                                feedId = feedId.takeIf { it != Routes.ALL_FEEDS },
                                onOpenArticle = { navController.navigate(Routes.reader(it)) },
                                onBack = { navController.popBackStack() },
                            )
                        }
                        composable(
                            Routes.READER,
                            arguments =
                                listOf(
                                    navArgument(Routes.READER_ARG_ARTICLE_ID) {
                                        type = NavType.LongType
                                    },
                                ),
                        ) { entry ->
                            val articleId =
                                entry.arguments?.getLong(Routes.READER_ARG_ARTICLE_ID) ?: return@composable
                            ReaderScreen(
                                articleId = articleId,
                                onBack = { navController.popBackStack() },
                            )
                        }
                        composable(Routes.SETTINGS) {
                            SettingsScreen(onBack = { navController.popBackStack() })
                        }
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
    }

    companion object {
        const val EXTRA_ARTICLE_ID = "article_id"
    }
}
