// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Media
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.StremioAddon
import app.tatakai.next.model.StremioAddonRepository
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import okhttp3.OkHttpClient
import java.io.File

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker r, checklist ext.stremio / ext.stremio.manage, plan §7
// D4): native Stremio addons. Signatures FINAL (the model interfaces); bodies: worker r.
//
// Plan for r: `install` fetches `<manifestUrl>` (also `stremio://host/manifest.json` -> https), validates `id`, `name`, `version`,
// `resources` contains `stream` (or an object with name `stream`), stores the addon as JSON in [storeDir]/addons.json. Streams:
// `GET <base>/stream/<type>/<id>.json` where `<id>` is `kitsu:<kitsuId>:<episode>` when the addon lists `idPrefixes` with `kitsu`, else
// IMDb through the Cinemeta search (`https://v3-cinemeta.strem.io/catalog/series/top/search=<title>.json`) + `tt…:<season>:<episode>`;
// the title -> id match is cached per addon in [storeDir]/matches/<addonId>.json (`resetMatches` deletes it). Each stream becomes a
// web `SourceCandidate` (url / `behaviorHints.proxyHeaders.request`) or a torrent one (`infoHash` + `fileIdx`), `extId = "stremio:<id>"`,
// `extName = addon.name`, `label = stream.name ?: stream.title` (first line). HTTP budget per addon 15 s; failures logged under op
// `stremio` in ExtensionLogStore (extId `stremio:<id>`).

class StremioAddonRepositoryImpl(
    private val client: OkHttpClient,
    private val storeDir: File,
    private val logs: ExtensionLogStore? = null,
    private val clock: () -> Long = System::currentTimeMillis,
) : StremioAddonRepository {
    private val _addons = MutableStateFlow<List<StremioAddon>>(emptyList())
    override val addons: StateFlow<List<StremioAddon>> = _addons.asStateFlow()

    override suspend fun install(manifestUrl: String): Result<StremioAddon> =
        Result.failure(NotImplementedError("wave B worker r: StremioAddonRepositoryImpl.install"))

    override suspend fun setEnabled(id: String, enabled: Boolean) {
        _addons.value = _addons.value.map { if (it.id == id) it.copy(enabled = enabled) else it }
    }

    override suspend fun remove(id: String) {
        _addons.value = _addons.value.filterNot { it.id == id }
    }

    override suspend fun update(id: String): Result<StremioAddon> =
        Result.failure(NotImplementedError("wave B worker r: StremioAddonRepositoryImpl.update"))

    override suspend fun resetMatches(id: String) {}

    /** "Restablecer la app": forgets every addon and cached match. */
    suspend fun wipe() {
        _addons.value = emptyList()
        storeDir.deleteRecursively()
    }

    companion object {
        const val DIR_NAME: String = "stremio"
    }
}

/** The enabled Stremio addons as one more provider group of `ExtensionRepositoryImpl.resolveCandidates` (like the PC servers). */
class StremioCandidateSource(
    private val repository: StremioAddonRepositoryImpl,
    private val client: OkHttpClient,
    private val budgetMs: Long = ADDON_BUDGET_MS,
) : ExternalCandidateSource {
    override val available: Flow<Boolean> = repository.addons.map { list -> list.any { it.enabled && it.hasStreams } }

    override suspend fun candidates(media: Media, episode: Int): List<SourceCandidate> {
        // wave B (r): ask every enabled addon in parallel within [budgetMs]; never throw.
        return emptyList()
    }

    companion object {
        const val ADDON_BUDGET_MS: Long = 15_000
        const val EXT_ID_PREFIX: String = "stremio:"
    }
}
