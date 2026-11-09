// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.model.MediaKind
import okhttp3.OkHttpClient

/**
 * Parity wave B (16 §3.1, worker r, checklist metadata.providers.shikimori): Shikimori (`shikimori.one/api`, no key, ~5 req/s with the
 * mandatory `User-Agent`). Mapped by MAL id (Shikimori ids ARE MAL ids): alternative titles, score, screenshots. If the API no longer
 * answers when r implements it, r drops the provider with a note in KNOWN-ISSUES (plan 14 §0.1 #13). Stub: no capability yet.
 */
class ShikimoriProvider(private val client: OkHttpClient) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME, MediaKind.MANGA)

    // wave B (r): MEDIA (+ SEARCH) once implemented.
    override val capabilities: Set<Capability> = emptySet()

    companion object {
        const val ID: String = "shikimori"
        const val BASE_URL: String = "https://shikimori.one/api"
    }
}
