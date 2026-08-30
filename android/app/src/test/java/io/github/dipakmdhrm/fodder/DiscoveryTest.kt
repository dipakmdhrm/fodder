package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.data.feed.FeedKind
import io.github.dipakmdhrm.fodder.data.feed.extractFeedLinks
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Mirrors `core/src/discovery.rs`'s tests - fixture HTML, no network. */
class DiscoveryTest {
    private val base = "https://blog.example.com/index.html"

    @Test
    fun `finds multiple alternates and resolves relative hrefs`() {
        val html =
            """
            <html><head>
              <link rel="alternate" type="application/rss+xml" title="RSS" href="/feed.xml">
              <link rel="alternate" type="application/atom+xml" title="Atom" href="atom.xml">
            </head><body></body></html>
            """.trimIndent()

        val found = extractFeedLinks(html, base)
        assertEquals(2, found.size)
        assertEquals("https://blog.example.com/feed.xml", found[0].url)
        assertEquals(FeedKind.RSS, found[0].kind)
        assertEquals("RSS", found[0].title)
        assertEquals("https://blog.example.com/atom.xml", found[1].url)
        assertEquals(FeedKind.ATOM, found[1].kind)
    }

    @Test
    fun `recognizes json feed`() {
        val html =
            """<link rel="alternate" type="application/feed+json" href="https://e.com/feed.json">"""
        val found = extractFeedLinks(html, base)
        assertEquals(1, found.size)
        assertEquals(FeedKind.JSON, found[0].kind)
    }

    @Test
    fun `ignores non-feed alternates`() {
        val html =
            """
            <link rel="alternate" type="text/html" href="/print">
            <link rel="stylesheet" type="text/css" href="/style.css">
            <link rel="alternate" href="/no-type">
            """.trimIndent()
        assertTrue(extractFeedLinks(html, base).isEmpty())
    }

    @Test
    fun `matches a mime type carrying a charset`() {
        val html =
            """<link rel="alternate" type="application/rss+xml; charset=utf-8" href="/feed">"""
        assertEquals(1, extractFeedLinks(html, base).size)
    }

    @Test
    fun `matches a rel list containing alternate`() {
        val html = """<link rel="alternate home" type="application/rss+xml" href="/feed">"""
        assertEquals(1, extractFeedLinks(html, base).size)
    }

    @Test
    fun `mime matching tolerates case and whitespace`() {
        assertEquals(FeedKind.RSS, FeedKind.fromMime("Application/RSS+XML"))
        assertEquals(FeedKind.ATOM, FeedKind.fromMime("  application/atom+xml  "))
        assertEquals(null, FeedKind.fromMime("text/html"))
    }
}
