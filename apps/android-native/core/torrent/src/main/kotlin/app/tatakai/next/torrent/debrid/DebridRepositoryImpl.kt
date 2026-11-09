// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.debrid

import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.torrent.DebridAccount
import app.tatakai.next.model.torrent.DebridFile
import app.tatakai.next.model.torrent.DebridRepository
import app.tatakai.next.model.torrent.DebridService
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import okhttp3.OkHttpClient

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.9, 2026-11-09 amendment). Owner: worker q.
 *
 * One client per service behind the same interface (classic `services/debrid/*`): Real-Debrid (`/rest/1.0/torrents/instantAvailability`,
 * `/torrents/addMagnet`, `/torrents/selectFiles`, `/unrestrict/link`), TorBox (`/v1/api/torrents/checkcached`, `/createtorrent`,
 * `/requestdl`), AllDebrid (`/v4/magnet/instant`, `/magnet/upload`, `/link/unlock`), Premiumize (`/api/cache/check`,
 * `/api/transfer/directdl`). API keys come from [SecretStore] (`SecretKeys.debridApiKey`), never from settings; the accounts list is
 * rebuilt from the stored keys at start. [client] is the shared app client (`networkModule`): no interceptor adds anything to these hosts.
 */
class DebridRepositoryImpl(private val client: OkHttpClient, private val secrets: SecretStore) : DebridRepository {
    override val accounts: StateFlow<List<DebridAccount>> = MutableStateFlow(emptyList())

    override suspend fun link(service: DebridService, apiKey: String): Result<DebridAccount> = TODO("worker q")

    override suspend fun unlink(service: DebridService): Unit = TODO("worker q")

    override suspend fun cached(infoHashes: List<String>): Map<String, Set<DebridService>> = emptyMap()

    override suspend fun files(service: DebridService, infoHash: String, magnet: String?): Result<List<DebridFile>> = TODO("worker q")

    override suspend fun resolve(service: DebridService, infoHash: String, magnet: String?, fileIndex: Int?, episode: Int?): Result<StreamSource> =
        TODO("worker q")
}
