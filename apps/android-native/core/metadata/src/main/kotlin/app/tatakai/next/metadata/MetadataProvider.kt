package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef

enum class Capability { FEED, SEARCH, MEDIA, EPISODES }

/**
 * A metadata source. Providers throw [ProviderException] for transport / HTTP failures (the router trips the
 * cool-down on those) and return empty values for "nothing found" (the router then tries the next provider).
 */
interface MetadataProvider {
    val id: String
    val kinds: Set<MediaKind>
    val capabilities: Set<Capability>

    suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage = unsupported("feed")
    suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage = unsupported("search")
    suspend fun media(kind: MediaKind, ref: MediaRef): Media? = unsupported("media")
    suspend fun episodes(ref: MediaRef): List<Episode> = unsupported("episodes")

    private fun <T> unsupported(what: String): T = throw UnsupportedOperationException("$id does not support $what")
}

/** [status] 0 = transport failure (offline, DNS, timeout); otherwise the HTTP status. */
class ProviderException(
    val provider: String,
    val status: Int,
    message: String,
    cause: Throwable? = null,
) : Exception("$provider: $message", cause) {
    /** Only transport/HTTP failures take a provider out of rotation; a bad payload for one title must not. */
    val tripsCooldown: Boolean
        get() = status == 0 || status == 401 || status == 403 || status == 429 || status >= 500
}

class NoMetadataProviderException(
    val capability: Capability,
    val errors: List<Pair<String, Throwable>>,
) : Exception(
    if (errors.isEmpty()) "No enabled metadata source supports $capability"
    else "All metadata sources failed for $capability: " + errors.joinToString("; ") { "${it.first}: ${it.second.message}" },
) {
    /** The provider that answered 429, if any (maps to AppError.RateLimited). */
    val rateLimitedProvider: String?
        get() = errors.firstNotNullOfOrNull { (id, e) -> id.takeIf { (e as? ProviderException)?.status == 429 } }

    val allOffline: Boolean
        get() = errors.isNotEmpty() && errors.all { (it.second as? ProviderException)?.status == 0 }
}
