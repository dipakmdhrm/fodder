package io.github.dipakmdhrm.fodder.work

/**
 * Build the one-line summary shown when a refresh finishes, e.g.
 * `"Up to date - 1.8s"`, `"3 new articles - 2.4s"`, or
 * `"1 new article - 1 error - 0.6s"`.
 *
 * Port of `core/src/refresh.rs::format_refresh_summary`, which feeds the desktop
 * viewer's completion toast; here it feeds the snackbar after a pull-to-refresh.
 * The only intentional difference is the separator: a plain hyphen rather than
 * the desktop's middle dot.
 *
 * @param newArticles genuinely-new items inserted across the polled feed(s).
 * @param errors feeds whose poll hard-errored (rate limits and 304s are not errors).
 * @param durationMs wall-clock time the refresh took.
 */
fun formatRefreshSummary(
    newArticles: Int,
    errors: Int,
    durationMs: Long,
): String {
    val parts = mutableListOf<String>()

    parts +=
        when (newArticles) {
            0 -> "Up to date"
            1 -> "1 new article"
            else -> "$newArticles new articles"
        }

    when (errors) {
        0 -> Unit
        1 -> parts += "1 error"
        else -> parts += "$errors errors"
    }

    parts += formatDuration(durationMs)

    return parts.joinToString(" - ")
}

/**
 * Format a poll duration for display. Sub-second and multi-second refreshes read
 * as `"0.6s"` / `"2.4s"`; anything past a minute switches to `"1m03s"`.
 */
private fun formatDuration(durationMs: Long): String =
    if (durationMs >= 60_000) {
        val secs = durationMs / 1000
        "%dm%02ds".format(secs / 60, secs % 60)
    } else {
        "%.1fs".format(durationMs / 1000.0)
    }
