package io.github.dipakmdhrm.fodder.data.http

import okhttp3.OkHttpClient
import okhttp3.Request
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.minutes
import kotlin.time.Duration.Companion.seconds

/**
 * Outcome of a single conditional GET, before any feed parsing.
 *
 * Port of `core/src/poller/http.rs::FetchResponse`.
 */
sealed interface FetchResponse {
    /** Server returned 304 - our cached copy is current. */
    data object NotModified : FetchResponse

    /** Server returned a body. Carries the fresh validators to store. */
    data class Modified(
        val etag: String?,
        val lastModified: String?,
        val body: ByteArray,
    ) : FetchResponse {
        // ByteArray gets identity equals/hashCode by default, which would make
        // this data class lie. Compare the bytes instead.
        override fun equals(other: Any?): Boolean {
            if (this === other) return true
            if (other !is Modified) return false
            return etag == other.etag &&
                lastModified == other.lastModified &&
                body.contentEquals(other.body)
        }

        override fun hashCode(): Int {
            var result = etag?.hashCode() ?: 0
            result = 31 * result + (lastModified?.hashCode() ?: 0)
            result = 31 * result + body.contentHashCode()
            return result
        }
    }

    /** Server asked us to back off (429, or 503 with Retry-After). */
    data class RateLimited(val retryAfter: Duration) : FetchResponse

    /** Transport error or non-success status. */
    data class Error(val message: String) : FetchResponse
}

/** Backoff used when a server rate-limits without a Retry-After hint. */
private val DEFAULT_RATE_LIMIT_BACKOFF = 5.minutes

/**
 * Perform a conditional GET for [url], replaying stored [etag] / [lastModified]
 * as `If-None-Match` / `If-Modified-Since` and classifying the response.
 *
 * Blocking: callers run it on a worker dispatcher.
 */
fun conditionalGet(
    client: OkHttpClient,
    url: String,
    etag: String? = null,
    lastModified: String? = null,
): FetchResponse {
    val request =
        Request.Builder()
            .url(url)
            .apply {
                etag?.takeIf { it.isNotBlank() }?.let { header("If-None-Match", it) }
                lastModified?.takeIf { it.isNotBlank() }?.let { header("If-Modified-Since", it) }
            }
            .build()

    return try {
        client.newCall(request).execute().use { response ->
            when {
                response.code == 304 -> FetchResponse.NotModified

                response.code == 429 || response.code == 503 -> {
                    val retryAfter =
                        response.header("Retry-After")?.let { parseRetryAfter(it) }
                            ?: DEFAULT_RATE_LIMIT_BACKOFF
                    FetchResponse.RateLimited(retryAfter)
                }

                !response.isSuccessful -> FetchResponse.Error("HTTP ${response.code}")

                else ->
                    FetchResponse.Modified(
                        etag = response.header("ETag"),
                        lastModified = response.header("Last-Modified"),
                        body = response.body?.bytes() ?: ByteArray(0),
                    )
            }
        }
    } catch (e: Exception) {
        FetchResponse.Error("request failed: ${e.message ?: e::class.java.simpleName}")
    }
}

/**
 * Parse a `Retry-After` header value, which is either an integer number of
 * seconds or an HTTP-date (IMF-fixdate). Returns the delay from [now].
 *
 * Port of `core/src/poller/http.rs::parse_retry_after`. A date in the past
 * clamps to zero rather than going negative.
 */
fun parseRetryAfter(
    value: String,
    now: Long = System.currentTimeMillis(),
): Duration? {
    val raw = value.trim()
    if (raw.isEmpty()) return null

    raw.toLongOrNull()?.let { return it.coerceAtLeast(0).seconds }

    val fmt = DateTimeFormatter.ofPattern("EEE, dd MMM yyyy HH:mm:ss 'GMT'", Locale.US)
    return runCatching {
        val target = ZonedDateTime.parse(raw, fmt.withZone(ZoneOffset.UTC))
        (target.toInstant().toEpochMilli() - now).coerceAtLeast(0).milliseconds
    }.getOrNull()
}
