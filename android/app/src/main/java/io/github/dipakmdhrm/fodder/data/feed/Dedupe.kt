package io.github.dipakmdhrm.fodder.data.feed

import java.security.MessageDigest

/**
 * Stable per-item identity for deduplication.
 *
 * Port of `core/src/poller/dedupe.rs`. Prefer the feed-provided id/GUID; when it
 * is absent, synthesize a stable key from a SHA-256 of the item's link and title
 * so the same item hashes the same on every poll and never re-notifies.
 *
 * The rule is kept byte-identical to the desktop app on purpose: the two stores
 * are independent, but a shared rule means one documented dedupe behavior.
 */
fun stableGuid(
    id: String?,
    link: String?,
    title: String?,
): String {
    val trimmed = id?.trim().orEmpty()
    if (trimmed.isNotEmpty()) return trimmed
    return hashLinkTitle(link.orEmpty(), title.orEmpty())
}

/** SHA-256 of `link` + `\n` + `title`, hex-encoded. Deterministic and stable. */
fun hashLinkTitle(
    link: String,
    title: String,
): String {
    val digest = MessageDigest.getInstance("SHA-256")
    digest.update(link.toByteArray(Charsets.UTF_8))
    digest.update('\n'.code.toByte())
    digest.update(title.toByteArray(Charsets.UTF_8))
    return digest.digest().joinToString("") { "%02x".format(it) }
}
