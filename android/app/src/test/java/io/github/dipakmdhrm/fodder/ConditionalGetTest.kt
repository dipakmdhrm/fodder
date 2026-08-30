package io.github.dipakmdhrm.fodder

import io.github.dipakmdhrm.fodder.data.http.FetchResponse
import io.github.dipakmdhrm.fodder.data.http.conditionalGet
import io.github.dipakmdhrm.fodder.data.http.parseRetryAfter
import okhttp3.OkHttpClient
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.time.Duration.Companion.seconds

/**
 * The Android counterpart of `core/tests/conditional_get.rs`: the conditional
 * headers are actually sent, and every response class maps to the right outcome.
 */
class ConditionalGetTest {
    private lateinit var server: MockWebServer
    private val client = OkHttpClient()

    @Before
    fun setUp() {
        server = MockWebServer()
        server.start()
    }

    @After
    fun tearDown() {
        server.shutdown()
    }

    @Test
    fun `stored validators are replayed as conditional headers`() {
        server.enqueue(MockResponse().setResponseCode(304))

        conditionalGet(client, server.url("/feed").toString(), "\"abc\"", "Wed, 21 Oct 2020 07:28:00 GMT")

        val recorded = server.takeRequest()
        assertEquals("\"abc\"", recorded.getHeader("If-None-Match"))
        assertEquals("Wed, 21 Oct 2020 07:28:00 GMT", recorded.getHeader("If-Modified-Since"))
    }

    @Test
    fun `304 is not modified`() {
        server.enqueue(MockResponse().setResponseCode(304))
        val result = conditionalGet(client, server.url("/feed").toString())
        assertTrue(result is FetchResponse.NotModified)
    }

    @Test
    fun `a 200 captures the body and the fresh validators`() {
        server.enqueue(
            MockResponse()
                .setResponseCode(200)
                .setHeader("ETag", "\"v2\"")
                .setHeader("Last-Modified", "Wed, 21 Oct 2025 07:28:00 GMT")
                .setBody("<rss></rss>"),
        )

        val result = conditionalGet(client, server.url("/feed").toString()) as FetchResponse.Modified
        assertEquals("\"v2\"", result.etag)
        assertEquals("Wed, 21 Oct 2025 07:28:00 GMT", result.lastModified)
        assertEquals("<rss></rss>", result.body.toString(Charsets.UTF_8))
    }

    @Test
    fun `429 with seconds is a rate limit`() {
        server.enqueue(MockResponse().setResponseCode(429).setHeader("Retry-After", "120"))
        val result = conditionalGet(client, server.url("/feed").toString()) as FetchResponse.RateLimited
        assertEquals(120.seconds, result.retryAfter)
    }

    @Test
    fun `503 without a hint still backs off`() {
        server.enqueue(MockResponse().setResponseCode(503))
        val result = conditionalGet(client, server.url("/feed").toString()) as FetchResponse.RateLimited
        assertTrue(result.retryAfter > 0.seconds)
    }

    @Test
    fun `other non-success statuses are errors`() {
        server.enqueue(MockResponse().setResponseCode(404))
        val result = conditionalGet(client, server.url("/feed").toString()) as FetchResponse.Error
        assertTrue(result.message.contains("404"))
    }

    @Test
    fun `retry-after accepts seconds`() {
        assertEquals(120.seconds, parseRetryAfter("120"))
        assertEquals(0.seconds, parseRetryAfter("  0 "))
    }

    @Test
    fun `retry-after accepts an http date`() {
        val now = System.currentTimeMillis()
        val future =
            ZonedDateTime.now(ZoneOffset.UTC).plusMinutes(5).format(
                DateTimeFormatter.ofPattern("EEE, dd MMM yyyy HH:mm:ss 'GMT'", Locale.US),
            )
        val parsed = parseRetryAfter(future, now)!!
        assertTrue(parsed > 0.seconds)
    }

    @Test
    fun `a retry-after in the past clamps to zero rather than going negative`() {
        val past =
            ZonedDateTime.now(ZoneOffset.UTC).minusHours(1).format(
                DateTimeFormatter.ofPattern("EEE, dd MMM yyyy HH:mm:ss 'GMT'", Locale.US),
            )
        assertEquals(0.seconds, parseRetryAfter(past))
    }

    @Test
    fun `retry-after garbage is null`() {
        assertNull(parseRetryAfter("soon"))
        assertNull(parseRetryAfter(""))
    }
}
