package io.github.dipakmdhrm.fodder.reader

import org.jsoup.Jsoup
import org.jsoup.nodes.Element
import org.jsoup.nodes.Node
import org.jsoup.nodes.TextNode
import org.jsoup.safety.Safelist

/** One run of styled text produced by the light reader. */
data class ReaderSpan(
    val text: String,
    val bold: Boolean = false,
    val italic: Boolean = false,
    val monospace: Boolean = false,
    val heading: Boolean = false,
    /** Set when this run came from an `<a href>`, so the UI can make it tappable. */
    val link: String? = null,
)

/**
 * The light reader: article HTML in, a flat list of styled runs out.
 *
 * Port of `fodder/src/reader.rs`, which sanitizes with `ammonia` and then walks
 * the DOM down to the handful of tags Pango understands. Here jsoup's [Safelist]
 * does the sanitizing and the walk produces [ReaderSpan]s the Compose layer
 * turns into an AnnotatedString.
 *
 * No scripts and no network: every `<script>`, `<style>`, `<iframe>`, and
 * every image is dropped before the walk starts, so nothing in an article can
 * phone home. That is the whole point of the light renderer - the WebView is the
 * opt-in for the live page.
 */
fun htmlToSpans(html: String): List<ReaderSpan> {
    val safe = Jsoup.clean(html, Safelist.basic().addTags("h1", "h2", "h3", "h4", "pre"))
    val body = Jsoup.parseBodyFragment(safe).body()

    val out = mutableListOf<ReaderSpan>()
    walk(body, Style(), out)
    return out.filter { it.text.isNotEmpty() }
}

/** Convenience for callers that only need the stripped text. */
fun htmlToPlainText(html: String): String = htmlToSpans(html).joinToString("") { it.text }

private data class Style(
    val bold: Boolean = false,
    val italic: Boolean = false,
    val monospace: Boolean = false,
    val heading: Boolean = false,
    val link: String? = null,
)

private val BLOCK_TAGS =
    setOf("p", "div", "blockquote", "li", "h1", "h2", "h3", "h4", "h5", "h6", "pre", "br")

private fun walk(
    node: Node,
    style: Style,
    out: MutableList<ReaderSpan>,
) {
    for (child in node.childNodes()) {
        when (child) {
            is TextNode -> {
                // jsoup has already resolved entities, so `&amp;` arrives as `&`.
                val text = child.wholeText.replace(Regex("\\s+"), " ")
                if (text.isNotBlank() || out.isNotEmpty()) {
                    out += ReaderSpan(text, style.bold, style.italic, style.monospace, style.heading, style.link)
                }
            }

            is Element -> {
                val tag = child.tagName().lowercase()
                val childStyle =
                    when (tag) {
                        "b", "strong" -> style.copy(bold = true)
                        "i", "em" -> style.copy(italic = true)
                        "code", "pre" -> style.copy(monospace = true)
                        "h1", "h2", "h3", "h4", "h5", "h6" -> style.copy(bold = true, heading = true)
                        "a" -> style.copy(link = child.attr("href").takeIf { it.isNotBlank() })
                        else -> style
                    }

                if (tag == "br") {
                    out += ReaderSpan("\n")
                    continue
                }

                walk(child, childStyle, out)

                if (tag in BLOCK_TAGS) {
                    out += ReaderSpan("\n\n")
                }
            }
        }
    }
}
