// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.ListProvider
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

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
