// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model.torrent

import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave A contract, 2026-11-09 amendment (plan 14 §0.1 #11): debrid moves to Android worker q in wave A. Desktop counterpart:
// tk-core/debrid/** (worker i), `DebridServiceVm` / `DebridStatusVm` of the desktop contract §3. Signatures FINAL; bodies: worker q in
// :core:torrent (`DebridRepositoryImpl`, bound in `torrentModule`). User decision: debrid is a section of the Torrents client
// (`TorrentSection.DEBRID`) plus `SourceCandidate.debrid: DebridStatus?`; no new SourceKind.

/** The four services of the classic app. [id] = desktop `DebridServiceVm::id()`: persistence / JSON key and `SecretKeys.debridApiKey`. */
@Serializable
enum class DebridService(val id: String, val label: String) {
    @SerialName("real_debrid") REAL_DEBRID("real_debrid", "Real-Debrid"),
    @SerialName("torbox") TORBOX("torbox", "TorBox"),
    @SerialName("alldebrid") ALLDEBRID("alldebrid", "AllDebrid"),
    @SerialName("premiumize") PREMIUMIZE("premiumize", "Premiumize"),
}

/** Desktop `DebridStatusVm`: a torrent source as seen by a linked debrid service. [cached] = instantly available over HTTPS. */
@Serializable
data class DebridStatus(val service: DebridService, val cached: Boolean)

/** A linked debrid account (the API key itself lives in `SecretStore` under `SecretKeys.debridApiKey`). */
data class DebridAccount(
    val service: DebridService,
    val username: String?,
    /** Epoch ms when premium ends; null = unknown / free. */
    val premiumUntilMs: Long?,
    val linkedAtMs: Long,
)

/** A torrent file the debrid service can serve directly over HTTPS. */
data class DebridFile(val index: Int, val name: String, val sizeBytes: Long, val episode: Int? = null)

/**
 * Debrid services: cached-torrent lookup, HTTPS resolution of a cached torrent, account linking by API key (classic Settings ›
 * Debrid). A cached torrent plays as a plain HTTPS stream (no libtorrent, no background gate); the source picker shows the
 * "En caché · <service>" badge (checklist sources.debrid.cached-badge) and the ranking prefers cached torrents over raw ones
 * (sources.debrid.ranking, worker q in :core:release `score`, additive). Hosts: `api.real-debrid.com`, `api.torbox.app`,
 * `api.alldebrid.com`, `www.premiumize.me` only, with the shared OkHttpClient; the API key travels only to its own service.
 */
interface DebridRepository {
    val accounts: StateFlow<List<DebridAccount>>

    /** Validates [apiKey] against the service (`/user`), stores it in `SecretStore`, returns the account. */
    suspend fun link(service: DebridService, apiKey: String): Result<DebridAccount>

    suspend fun unlink(service: DebridService)

    /** Which linked services have each info-hash cached (instant availability). Unknown / unlinked = absent. Batched, short timeout. */
    suspend fun cached(infoHashes: List<String>): Map<String, Set<DebridService>>

    /**
     * Fills [SourceCandidate.debrid] of the torrent candidates from [cached] (first linked service that has it; `cached = false` for
     * linked services that do not). Called by the player's `SourceFlow` (worker p, one line) before ranking; without linked accounts
     * it returns [candidates] untouched. Default here so `:core:player` needs no dependency on `:core:torrent`.
     */
    suspend fun markCached(candidates: List<SourceCandidate>): List<SourceCandidate> = candidates

    /** Files of a cached torrent as the service lists them (adds the magnet to the account when needed). */
    suspend fun files(service: DebridService, infoHash: String, magnet: String?): Result<List<DebridFile>>

    /**
     * Direct HTTPS stream of one file of a cached torrent (`Container.PROGRESSIVE`, `provider = service.label`, `extensionId` of the
     * torrent source). Fails with `AppException(AppError.Http(status))` when the service rejects it.
     */
    suspend fun resolve(service: DebridService, infoHash: String, magnet: String?, fileIndex: Int?, episode: Int?): Result<StreamSource>
}
