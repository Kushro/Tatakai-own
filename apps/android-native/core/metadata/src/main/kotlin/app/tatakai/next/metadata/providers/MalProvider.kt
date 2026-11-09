// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.model.MediaKind
import okhttp3.OkHttpClient

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r, checklist metadata.providers.mal): MyAnimeList
 * (`api.myanimelist.net/v2`, `X-MAL-CLIENT-ID` = the same `TATAKAI_MAL_CLIENT_ID` the lists use; without it the provider reports
 * `ProviderException(status = 401)` and the router skips it). Fills score, rank and synopsis when AniList has none; resolves by
 * `MediaRef.malId` (ani.zip mapping when only the AniList id is known). Stub: no capability until r implements, so the router never
 * routes here even when the user enables "mal" in Settings › Metadatos.
 */
class MalProvider(
    private val client: OkHttpClient,
    /** Koin property PROP_MAL_CLIENT_ID (same id as the lists OAuth); null / blank = provider unavailable. */
    private val clientId: () -> String? = { null },
) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME, MediaKind.MANGA)

    // wave B (r): add MEDIA (+ SEARCH / FEED if the public API allows it without a user token) once implemented.
    override val capabilities: Set<Capability> = emptySet()

    companion object {
        const val ID: String = "mal"
        const val BASE_URL: String = "https://api.myanimelist.net/v2"
    }
}
