package io.github.dipakmdhrm.fodder.data.feed

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import org.jsoup.Jsoup
import org.jsoup.nodes.Document
import org.jsoup.nodes.Element
import org.jsoup.parser.Parser
import java.time.OffsetDateTime
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

/** A freshly-parsed item, before it is assigned a row id. */
data class ParsedItem(
    val guid: String,
    val title: String,
    val url: String?,
    val content: String?,
    val publishedAt: Long?,
)

/** A parsed feed document: the channel title (when present) and its items. */
data class ParsedFeed(
    val title: String?,
    val items: List<ParsedItem>,
)

/**
 * Parse raw feed bytes into a title and item list, covering RSS 2.0, Atom, and
 * JSON Feed.
 *
 * Port of `core/src/poller/mod.rs::parse_items`, which delegates format
 * detection to `feed-rs`. There is no equivalent library on Android worth the
 * dependency, so the three formats are handled directly here - XML through
 * jsoup's XML parser (already a dependency for the reader), JSON Feed through
 * kotlinx.serialization.
 *
 * Returns null when the bytes are not a feed at all, which is how feed discovery
 * decides "this URL is a page, not a feed".
 */
fun parseFeed(bytes: ByteArray): ParsedFeed? {
    val text = bytes.toString(Charsets.UTF_8).trim()
    if (text.isEmpty()) return null

    if (text.startsWith("{")) return parseJsonFeed(text)

    val doc = Jsoup.parse(text, "", Parser.xmlParser())
    return when {
        doc.selectFirst("rss") != null || doc.selectFirst("channel") != null -> parseRss(doc)
        doc.selectFirst("feed") != null -> parseAtom(doc)
        else -> null
    }
}

private fun parseRss(doc: Document): ParsedFeed {
    val title = doc.selectFirst("channel")?.childText("title")

    val items =
        doc.getElementsByTag("item").map { item ->
            val link = item.childText("link")
            val itemTitle = item.childText("title").orEmpty()
            // <content:encoded> carries the full body when a feed ships one;
            // <description> is usually just the summary.
            val content = item.childText("content:encoded") ?: item.childText("description")
            ParsedItem(
                guid = stableGuid(item.childText("guid"), link, itemTitle),
                title = itemTitle,
                url = link,
                content = content,
                publishedAt =
                    parseFeedDate(item.childText("pubDate"))
                        ?: parseFeedDate(item.childText("dc:date")),
            )
        }
    return ParsedFeed(title, items)
}

private fun parseAtom(doc: Document): ParsedFeed {
    val title = doc.selectFirst("feed")?.childText("title")

    val items =
        doc.getElementsByTag("entry").map { entry ->
            val link = entry.atomLink()
            val entryTitle = entry.childText("title").orEmpty()
            val content = entry.childText("content") ?: entry.childText("summary")
            ParsedItem(
                guid = stableGuid(entry.childText("id"), link, entryTitle),
                title = entryTitle,
                url = link,
                content = content,
                publishedAt =
                    parseFeedDate(entry.childText("published"))
                        ?: parseFeedDate(entry.childText("updated")),
            )
        }
    return ParsedFeed(title, items)
}

private fun parseJsonFeed(text: String): ParsedFeed? {
    val root = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
    // A JSON document without `items` is some other API response, not a feed.
    val items = root["items"] as? JsonArray ?: return null

    val parsed =
        items.filterIsInstance<JsonObject>().map { item ->
            val url = item.str("url")
            val itemTitle = item.str("title").orEmpty()
            val content = item.str("content_html") ?: item.str("content_text")
            ParsedItem(
                guid = stableGuid(item.str("id"), url, itemTitle),
                title = itemTitle,
                url = url,
                content = content,
                publishedAt =
                    parseFeedDate(item.str("date_published"))
                        ?: parseFeedDate(item.str("date_modified")),
            )
        }
    return ParsedFeed(root.str("title"), parsed)
}

/**
 * Parse the date forms feeds actually use: RFC 3339 / ISO 8601 (Atom, JSON Feed)
 * and RFC 822 / 1123 (RSS `pubDate`). Returns epoch millis, or null when the
 * value is absent or unparseable - a missing date is never worth failing a poll
 * over, so callers fall back to the time the item was first seen.
 */
fun parseFeedDate(value: String?): Long? {
    val raw = value?.trim().orEmpty()
    if (raw.isEmpty()) return null

    tryParse { OffsetDateTime.parse(raw).toInstant().toEpochMilli() }?.let { return it }
    tryParse {
        ZonedDateTime.parse(raw, DateTimeFormatter.RFC_1123_DATE_TIME).toInstant().toEpochMilli()
    }?.let { return it }
    // Some feeds emit an RFC 822 variant with a zone name that
    // RFC_1123_DATE_TIME rejects (e.g. "Mon, 01 Jan 2024 09:00:00 PST").
    tryParse {
        val fmt = DateTimeFormatter.ofPattern("EEE, dd MMM yyyy HH:mm:ss zzz", Locale.US)
        ZonedDateTime.parse(raw, fmt).toInstant().toEpochMilli()
    }?.let { return it }
    return null
}

private inline fun tryParse(block: () -> Long): Long? = runCatching(block).getOrNull()

/** Direct child text by tag name, or null when the child is missing or empty. */
private fun Element.childText(tag: String): String? =
    children()
        .firstOrNull { it.tagName().equals(tag, ignoreCase = true) }
        ?.text()
        ?.trim()
        ?.takeIf { it.isNotEmpty() }

/**
 * Atom entries carry links as attributes. Prefer `rel="alternate"` (the human
 * page); fall back to a link with no `rel` at all, then to whatever is there.
 */
private fun Element.atomLink(): String? {
    val links = children().filter { it.tagName().equals("link", ignoreCase = true) }
    val preferred =
        links.firstOrNull { it.attr("rel") == "alternate" }
            ?: links.firstOrNull { it.attr("rel").isEmpty() }
            ?: links.firstOrNull()
    return preferred?.attr("href")?.trim()?.takeIf { it.isNotEmpty() }
}

/** String field of a JSON object, or null when absent, null, or empty. */
private fun JsonObject.str(key: String): String? =
    (this[key] as? JsonPrimitive)?.contentOrNull?.takeIf { it.isNotEmpty() }
