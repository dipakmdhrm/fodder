package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.data.feed.hashLinkTitle
import io.github.dipakmdhrm.fodder.data.feed.stableGuid
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test

/** Mirrors `core/src/poller/dedupe.rs`'s tests. */
class DedupeTest {
    @Test
    fun `a present id wins`() {
        assertEquals("guid-1", stableGuid("guid-1", "https://e.com/1", "First"))
    }

    @Test
    fun `a blank id falls back to the hash`() {
        val hashed = stableGuid("   ", "https://e.com/1", "First")
        assertEquals(hashLinkTitle("https://e.com/1", "First"), hashed)
        assertEquals(64, hashed.length)
    }

    @Test
    fun `a missing id falls back to the hash`() {
        assertEquals(
            hashLinkTitle("https://e.com/1", "First"),
            stableGuid(null, "https://e.com/1", "First"),
        )
    }

    @Test
    fun `the hash is deterministic and idempotent`() {
        val a = hashLinkTitle("https://e.com/1", "First")
        val b = hashLinkTitle("https://e.com/1", "First")
        assertEquals(a, b)
    }

    @Test
    fun `different links or titles hash differently`() {
        assertNotEquals(
            hashLinkTitle("https://e.com/1", "First"),
            hashLinkTitle("https://e.com/2", "First"),
        )
        assertNotEquals(
            hashLinkTitle("https://e.com/1", "First"),
            hashLinkTitle("https://e.com/1", "Second"),
        )
    }

    @Test
    fun `the separator keeps field boundaries distinct`() {
        // Without the newline between fields these two would collide.
        assertNotEquals(hashLinkTitle("ab", "c"), hashLinkTitle("a", "bc"))
    }

    @Test
    fun `missing link and title still produce a stable key`() {
        assertEquals(hashLinkTitle("", ""), stableGuid(null, null, null))
    }
}
