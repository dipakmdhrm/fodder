package io.github.dipakmdhrm.fodder.data.feed

import org.jsoup.Jsoup
import java.net.URI

/** The feed formats a `<link rel="alternate">` can point at. */
enum class FeedKind {
    RSS,
    ATOM,
    JSON,
    ;

    companion object {
        /**
         * Match a `type` attribute, tolerating a charset parameter
         * (`application/rss+xml; charset=utf-8`).
         */
        fun fromMime(mime: String): FeedKind? {
            val base = mime.substringBefore(';').trim().lowercase()
            return when (base) {
                "application/rss+xml", "text/rss+xml" -> RSS
                "application/atom+xml", "text/atom+xml" -> ATOM
                "application/feed+json", "application/json" -> JSON
                "application/xml", "text/xml" -> RSS
                else -> null
            }
        }
    }
}

/** A feed link found on a page. */
data class DiscoveredFeed(
    val url: String,
    val title: String?,
    val kind: FeedKind,
)

/** Outcome of resolving a user-entered URL. */
sealed interface DiscoveryResult {
    /** The URL was itself a feed. */
    data class Direct(val url: String, val title: String?) : DiscoveryResult

    /** The URL was a page advertising one or more feeds. */
    data class Candidates(val feeds: List<DiscoveredFeed>) : DiscoveryResult

    /** Neither a feed nor a page with feed links. */
    data object None : DiscoveryResult
}

/**
 * Extract `<link rel="alternate">` feed links from HTML, resolving relative
 * hrefs against [base].
 *
 * Port of `core/src/discovery.rs::extract_feed_links` - pure, so it is testable
 * on fixture HTML with no network.
 */
fun extractFeedLinks(
    html: String,
    base: String,
): List<DiscoveredFeed> {
    val doc = Jsoup.parse(html, base)
    // `rel~=alternate` matches rel lists that contain the word "alternate".
    return doc.select("link[rel~=(?i)alternate][type]").mapNotNull { el ->
        val kind = FeedKind.fromMime(el.attr("type")) ?: return@mapNotNull null
        val href = el.attr("href").trim().takeIf { it.isNotEmpty() } ?: return@mapNotNull null
        val resolved = resolveUrl(base, href) ?: return@mapNotNull null
        DiscoveredFeed(
            url = resolved,
            title = el.attr("title").trim().takeIf { it.isNotEmpty() },
            kind = kind,
        )
    }
}

/** Resolve a possibly-relative href against a base URL, or null if malformed. */
fun resolveUrl(
    base: String,
    href: String,
): String? = runCatching { URI(base).resolve(href).toString() }.getOrNull()
