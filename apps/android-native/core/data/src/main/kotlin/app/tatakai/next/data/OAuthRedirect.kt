// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.ListProvider
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import java.net.URLDecoder
import java.net.URLEncoder
import java.security.SecureRandom
import java.util.Base64

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Owner afterwards: worker q.

/** Koin property keys for the OAuth client ids; `:app` sets them from BuildConfig (`local.properties` / env, never committed). */
const val PROP_ANILIST_CLIENT_ID = "TATAKAI_ANILIST_CLIENT_ID"
const val PROP_MAL_CLIENT_ID = "TATAKAI_MAL_CLIENT_ID"

/**
 * The OAuth redirect of the list providers: `tatakai://oauth/<provider.id>?code=…&state=…`. Declared as an intent filter of
 * MainActivity (manifest of :app); registered as the redirect URI of the AniList and MAL API clients. Pure JVM (unit-testable).
 */
object OAuthRedirect {
    const val SCHEME = "tatakai"
    const val HOST = "oauth"

    fun redirectUri(provider: ListProvider): String = "$SCHEME://$HOST/${provider.id}"

    /** The provider a redirect URI belongs to; null when it is not one of ours. */
    fun providerOf(uri: String): ListProvider? {
        val prefix = "$SCHEME://$HOST/"
        if (!uri.startsWith(prefix, ignoreCase = true)) return null
        val id = uri.substring(prefix.length).takeWhile { it != '?' && it != '/' && it != '#' }.lowercase()
        return ListProvider.entries.firstOrNull { it.id == id }
    }

    /**
     * Query and fragment parameters of a redirect, URL-decoded (`?code=…&state=…` for authorization codes, `#access_token=…` for the
     * AniList implicit grant). On a repeated key the first value wins. Pure JVM (no android.net.Uri) so it is unit-testable.
     */
    fun params(uri: String): Map<String, String> {
        val query = uri.substringAfter('?', "").substringBefore('#')
        val fragment = uri.substringAfter('#', "")
        val out = LinkedHashMap<String, String>()
        for (part in (query.split('&') + fragment.split('&'))) {
            if (part.isBlank()) continue
            val name = decode(part.substringBefore('='))
            val value = decode(part.substringAfter('=', ""))
            if (name.isNotEmpty() && name !in out) out[name] = value
        }
        return out
    }

    private fun decode(s: String): String = runCatching { URLDecoder.decode(s, "UTF-8") }.getOrDefault(s)

    internal fun encode(s: String): String = URLEncoder.encode(s, "UTF-8")
}

/**
 * PKCE (RFC 7636) and the OAuth `state`. MAL only supports the `plain` method, so the challenge is the verifier itself
 * (`code_challenge_method=plain`). Verifier: 64 chars of the unreserved set (43..128 allowed).
 */
object Pkce {
    private const val UNRESERVED = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"
    private val random = SecureRandom()

    fun verifier(length: Int = 64): String {
        require(length in 43..128) { "PKCE verifier length must be 43..128" }
        return buildString(length) { repeat(length) { append(UNRESERVED[random.nextInt(UNRESERVED.length)]) } }
    }

    /** `plain` method: the challenge equals the verifier. */
    fun plainChallenge(verifier: String): String = verifier

    fun isValidVerifier(v: String): Boolean = v.length in 43..128 && v.all { it in UNRESERVED }

    /** 32 random bytes, base64url without padding. */
    fun state(): String = Base64.getUrlEncoder().withoutPadding().encodeToString(ByteArray(32).also(random::nextBytes))
}

/** An authorization started by `beginLink`, kept in `SecretStore` until the redirect arrives (the app may die meanwhile). */
@Serializable
data class PendingOAuth(val provider: String, val state: String, val verifier: String? = null, val createdAtMs: Long) {
    fun isExpired(nowMs: Long): Boolean = nowMs - createdAtMs > MAX_AGE_MS || nowMs < createdAtMs - 60_000

    companion object {
        /** A login left open longer than this must start again. */
        const val MAX_AGE_MS = 15L * 60 * 1000
    }
}

/** The OAuth token set of a list provider, stored as JSON under `SecretKeys.listToken(provider.id)`. */
@Serializable
data class TokenSet(val access: String, val refresh: String? = null, val expiresAtMs: Long? = null) {
    /** Expired, or expiring within [skewMs]. */
    fun isExpired(nowMs: Long, skewMs: Long = 60_000): Boolean = expiresAtMs != null && nowMs + skewMs >= expiresAtMs
}

/** Error codes of the OAuth / lists layer, carried as `AppError.Unknown(code)`; Settings › Listas y cuentas maps them to texts. */
object ListErrors {
    const val NO_CLIENT_ID = "oauth:no_client_id"
    const val CANCELLED = "oauth:cancelled"
    const val INVALID_STATE = "oauth:invalid_state"
    const val EXPIRED = "oauth:expired"
    const val EXCHANGE = "oauth:exchange"
    const val DENIED = "oauth:denied"
    const val NOT_LINKED = "lists:not_linked"
    const val NO_ID = "lists:no_id"
    const val NOT_ON_LIST = "lists:not_on_list"
    const val BAD_FILE = "lists:bad_file"
}

/**
 * A redirect opened from the Custom Tab waiting for [app.tatakai.next.model.ListsRepository.completeLink]. MainActivity offers it
 * (cold start and `onNewIntent`); the lists repository (worker q) collects [link] and finishes the exchange, then clears it with [consume].
 */
class PendingOAuthRedirect {
    private val _link = MutableStateFlow<String?>(null)
    val link: StateFlow<String?> = _link.asStateFlow()

    fun offer(raw: String) {
        _link.value = raw
    }

    /** Returns and clears the pending redirect. */
    fun consume(): String? = _link.value.also { _link.value = null }
}
