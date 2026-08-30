package io.github.dipakmdhrm.fodder.data.http

import kotlin.time.Duration
import kotlin.time.Duration.Companion.hours

/** Cap on retry delay, however many times a feed has failed in a row. */
private val CAP = 6.hours

/**
 * Next backoff delay for a feed after [errorCount] consecutive failures:
 * exponential from [base], doubling per failure, capped at six hours.
 *
 * Port of `core/src/poller/mod.rs::backoff_next`. The shift is clamped so a feed
 * that has been broken for months still produces a sane delay instead of
 * overflowing.
 */
fun backoffNext(
    errorCount: Int,
    base: Duration,
): Duration {
    val exponent = errorCount.coerceIn(0, 16)
    val factor = 1L shl exponent
    val scaled = base * factor.toDouble()
    return if (scaled > CAP) CAP else scaled
}
