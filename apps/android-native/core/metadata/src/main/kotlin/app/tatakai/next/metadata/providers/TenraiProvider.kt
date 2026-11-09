// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.model.MediaKind
import okhttp3.OkHttpClient

/**
 * Parity wave B (16 §3.1, worker r, checklist metadata.providers.tenrai): Tenrai, the last-resort provider of the classic router
 * (`src/lib/metadata/providers/tenrai.ts`) for titles AniList / Kitsu do not know. r copies the endpoint and the mapping from the classic
 * file; if the service no longer answers, r removes this provider (and its id from `AppSettings.KNOWN_PROVIDER_IDS`) with a note in
 * KNOWN-ISSUES (plan 14 §0.1 #13). Stub: no capability yet.
 */
class TenraiProvider(private val client: OkHttpClient) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME)

    // wave B (r): SEARCH + MEDIA once implemented (or delete the class).
    override val capabilities: Set<Capability> = emptySet()

    companion object {
        const val ID: String = "tenrai"
    }
}
