package io.github.dipakmdhrm.fodder.ui.nav

/**
 * The navigation graph, as plain string routes.
 *
 * The desktop shows feeds, articles, and the reader as three panes at once; a
 * phone shows them as three destinations. Route building is kept here (rather
 * than inline at each call site) so the argument names cannot drift apart.
 */
object Routes {
    const val FEEDS = "feeds"
    const val SETTINGS = "settings"

    const val ARTICLES_ARG_FEED_ID = "feedId"
    const val ARTICLES = "articles/{$ARTICLES_ARG_FEED_ID}"

    const val READER_ARG_ARTICLE_ID = "articleId"
    const val READER = "reader/{$READER_ARG_ARTICLE_ID}"

    /** `feedId` of -1 means "all articles", which is the start destination. */
    const val ALL_FEEDS = -1L

    fun articles(feedId: Long) = "articles/$feedId"

    fun reader(articleId: Long) = "reader/$articleId"
}
