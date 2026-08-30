package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.data.http.backoffNext
import io.github.dipakmdhrm.fodder.work.formatRefreshSummary
import org.junit.Assert.assertEquals
import org.junit.Test
import kotlin.time.Duration.Companion.hours
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

/** Mirrors `core/src/poller/mod.rs::backoff_next` and `core/src/refresh.rs`. */
class BackoffAndSummaryTest {
    @Test
    fun `backoff doubles per failure`() {
        val base = 60.seconds
        assertEquals(60.seconds, backoffNext(0, base))
        assertEquals(120.seconds, backoffNext(1, base))
        assertEquals(480.seconds, backoffNext(3, base))
    }

    @Test
    fun `backoff caps at six hours and never overflows`() {
        val base = 60.seconds
        assertEquals(6.hours, backoffNext(100, base))
        assertEquals(6.hours, backoffNext(Int.MAX_VALUE, base))
    }

    @Test
    fun `a negative error count is treated as zero`() {
        assertEquals(5.minutes, backoffNext(-1, 5.minutes))
    }

    @Test
    fun `summary reads up to date when nothing is new`() {
        assertEquals("Up to date - 1.8s", formatRefreshSummary(0, 0, 1800))
    }

    @Test
    fun `summary singularizes one article`() {
        assertEquals("1 new article - 0.6s", formatRefreshSummary(1, 0, 600))
    }

    @Test
    fun `summary pluralizes several articles`() {
        assertEquals("3 new articles - 2.4s", formatRefreshSummary(3, 0, 2400))
    }

    @Test
    fun `summary appends an error clause`() {
        assertEquals("1 new article - 1 error - 0.6s", formatRefreshSummary(1, 1, 600))
        assertEquals("Up to date - 2 errors - 0.5s", formatRefreshSummary(0, 2, 500))
    }

    @Test
    fun `durations past a minute switch format`() {
        assertEquals("Up to date - 1m03s", formatRefreshSummary(0, 0, 63_000))
        assertEquals("Up to date - 2m00s", formatRefreshSummary(0, 0, 120_000))
    }
}
