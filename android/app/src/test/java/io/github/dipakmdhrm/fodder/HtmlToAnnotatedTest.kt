package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.reader.htmlToPlainText
import io.github.dipakmdhrm.fodder.reader.htmlToSpans
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Mirrors `fodder/src/reader.rs`'s HTML tests. */
class HtmlToAnnotatedTest {
    @Test
    fun `scripts and styles are stripped`() {
        val text = htmlToPlainText("<p>before</p><script>alert('x')</script><p>after</p>")
        assertTrue(text.contains("before"))
        assertTrue(text.contains("after"))
        assertFalse(text.contains("alert"))
    }

    @Test
    fun `iframes and images cannot smuggle a network call`() {
        val html = """<p>hi</p><iframe src="https://tracker.example"></iframe>"""
        val spans = htmlToSpans(html)
        assertFalse(spans.any { it.text.contains("tracker.example") })
    }

    @Test
    fun `basic formatting is converted`() {
        val spans = htmlToSpans("<p><b>bold</b> and <i>italic</i> and <code>mono</code></p>")
        assertTrue(spans.any { it.text.contains("bold") && it.bold })
        assertTrue(spans.any { it.text.contains("italic") && it.italic })
        assertTrue(spans.any { it.text.contains("mono") && it.monospace })
    }

    @Test
    fun `headings are marked`() {
        val spans = htmlToSpans("<h2>A heading</h2><p>body</p>")
        assertTrue(spans.any { it.text.contains("A heading") && it.heading })
    }

    @Test
    fun `entities are decoded rather than shown raw`() {
        val text = htmlToPlainText("<p>Tom &amp; Jerry &lt;3</p>")
        assertTrue(text.contains("Tom & Jerry <3"))
    }

    @Test
    fun `safe links are kept with their href`() {
        val spans = htmlToSpans("""<p>see <a href="https://example.com/x">this</a></p>""")
        assertTrue(spans.any { it.link == "https://example.com/x" })
    }

    @Test
    fun `javascript hrefs are dropped`() {
        val spans = htmlToSpans("""<p><a href="javascript:alert(1)">tap</a></p>""")
        assertTrue(spans.any { it.text.contains("tap") })
        assertFalse(spans.any { it.link?.startsWith("javascript:") == true })
    }

    @Test
    fun `empty html yields no spans`() {
        assertTrue(htmlToSpans("").isEmpty())
    }
}
