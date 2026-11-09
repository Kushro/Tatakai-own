// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ProviderItem
import app.tatakai.next.model.StreamSource

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t; checklist ext.ecosystems.via-server,
 * server.api.invoke / server.api.extensions phone side): custom sources that do not come from an installed `.kai` - today the sources of
 * the linked PC extension servers (`:core:extension-server`, Aniyomi / Mihon / CloudStream / Sora / Mangayomi only through the server,
 * plan D4). `ExtensionRepositoryImpl.customSources` / `provider*` delegate every `extensionId` this group [owns] (`server:<serverId>/…`).
 *
 * Implementations bound their own time, never throw from [customSources] (a server that is down contributes nothing:
 * `ext.server-client.fallback`), and may throw from the other calls (the provider page shows its error state).
 */
interface ExternalCustomSources {
    /** True when [extensionId] belongs to this group. */
    fun owns(extensionId: String): Boolean

    suspend fun customSources(): List<CustomSourceRef>

    suspend fun home(extensionId: String, sourceId: String): List<ExtensionHomeRow>

    suspend fun search(extensionId: String, sourceId: String, query: String, page: Int): List<ExtensionHomeItem>

    suspend fun info(extensionId: String, sourceId: String, itemId: String): ProviderItem?

    suspend fun watch(extensionId: String, sourceId: String, itemId: String, entryId: String): List<StreamSource>

    suspend fun read(extensionId: String, sourceId: String, itemId: String, entryId: String): List<ChapterPage>
}
