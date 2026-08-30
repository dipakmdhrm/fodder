package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.data.feed.parseFeed
import io.github.dipakmdhrm.fodder.data.feed.parseFeedDate
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Mirrors `core/src/poller/mod.rs`'s parser tests: title plus items out of a
 * fixture document, for each of the three formats the app accepts.
 */
class FeedParserTest {
    private val rss =
        """
        <?xml version="1.0"?>
        <rss version="2.0"><channel>
          <title>Example Feed</title>
          <item><guid>guid-1</guid><title>First</title>
            <link>https://e.com/1</link><description>Body one</description></item>
          <item><guid>guid-2</guid><title>Second</title>
            <link>https://e.com/2</link><description>Body two</description></item>
        </channel></rss>
        """.trimIndent()

    @Test
    fun `parses rss title and items`() {
        val feed = parseFeed(rss.toByteArray())!!
        assertEquals("Example Feed", feed.title)
        assertEquals(2, feed.items.size)
        assertEquals("guid-1", feed.items[0].guid)
        assertEquals("First", feed.items[0].title)
        assertEquals("https://e.com/1", feed.items[0].url)
        assertEquals("Body one", feed.items[0].content)
    }

    @Test
    fun `prefers content encoded over description`() {
        val xml =
            """
            <rss version="2.0"><channel><title>T</title>
              <item><guid>g</guid><title>A</title><link>https://e.com/a</link>
                <description>short</description>
                <content:encoded>the full body</content:encoded></item>
            </channel></rss>
            """.trimIndent()
        val feed = parseFeed(xml.toByteArray())!!
        assertEquals("the full body", feed.items[0].content)
    }

    @Test
    fun `parses atom entries and prefers the alternate link`() {
        val xml =
            """
            <?xml version="1.0" encoding="utf-8"?>
            <feed xmlns="http://www.w3.org/2005/Atom">
              <title>Atom Example</title>
              <entry>
                <id>tag:e.com,2024:1</id>
                <title>Entry one</title>
                <link rel="self" href="https://e.com/feed"/>
                <link rel="alternate" href="https://e.com/post/1"/>
                <updated>2024-03-01T10:00:00Z</updated>
                <summary>a summary</summary>
              </entry>
            </feed>
            """.trimIndent()
        val feed = parseFeed(xml.toByteArray())!!
        assertEquals("Atom Example", feed.title)
        assertEquals(1, feed.items.size)
        assertEquals("tag:e.com,2024:1", feed.items[0].guid)
        assertEquals("https://e.com/post/1", feed.items[0].url)
        assertNotNull(feed.items[0].publishedAt)
    }

    @Test
    fun `parses json feed`() {
        val json =
            """
            {
              "version": "https://jsonfeed.org/version/1.1",
              "title": "JSON Example",
              "items": [
                {
                  "id": "1",
                  "url": "https://e.com/1",
                  "title": "JSON one",
                  "content_html": "<p>hi</p>",
                  "date_published": "2024-03-01T10:00:00Z"
                }
              ]
            }
            """.trimIndent()
        val feed = parseFeed(json.toByteArray())!!
        assertEquals("JSON Example", feed.title)
        assertEquals("1", feed.items[0].guid)
        assertEquals("<p>hi</p>", feed.items[0].content)
    }

    @Test
    fun `an item without a guid falls back to the hashed link and title`() {
        val xml =
            """
            <rss version="2.0"><channel><title>T</title>
              <item><title>No guid</title><link>https://e.com/x</link></item>
            </channel></rss>
            """.trimIndent()
        val first = parseFeed(xml.toByteArray())!!.items[0].guid
        val second = parseFeed(xml.toByteArray())!!.items[0].guid
        // 64 hex characters, and stable across parses so it never re-notifies.
        assertEquals(64, first.length)
        assertEquals(first, second)
    }

    @Test
    fun `html and json that are not feeds parse to null`() {
        assertNull(parseFeed("<html><body><p>hello</p></body></html>".toByteArray()))
        assertNull(parseFeed("""{"status":"ok"}""".toByteArray()))
        assertNull(parseFeed(ByteArray(0)))
    }

    @Test
    fun `parses the date forms feeds actually use`() {
        assertNotNull(parseFeedDate("2024-03-01T10:00:00Z"))
        assertNotNull(parseFeedDate("Fri, 01 Mar 2024 10:00:00 GMT"))
        assertNotNull(parseFeedDate("Fri, 01 Mar 2024 10:00:00 +0000"))
        assertNull(parseFeedDate("sometime last week"))
        assertNull(parseFeedDate(null))
        assertNull(parseFeedDate("  "))
    }

    @Test
    fun `rfc3339 and rfc822 forms of the same instant agree`() {
        val iso = parseFeedDate("2024-03-01T10:00:00Z")!!
        val rfc822 = parseFeedDate("Fri, 01 Mar 2024 10:00:00 GMT")!!
        assertTrue(iso == rfc822)
    }
}
