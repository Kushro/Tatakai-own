// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave A contract, 2026-11-09 amendment (plan 14 §0.1 #9): the watch history (exact resume position) syncs between the user's
// own devices over Tatakai Link, with no hosted backend. The PC (tk-link, desktop worker f; desktop contract §3.8) serves `/v1/history`
// behind the same mTLS + per-request device check as every other route; this phone pulls from a cursor and pushes its dirty rows.
// Wire shape = the camelCase JSON of `tk_core::model::HistoryEntry` (the E9 keys the Room entity already uses).
// Owner of this file afterwards: Android worker q (the only part of :core:link q edits, together with the `LinkHistoryRemote` binding
// in LinkModule.kt and the two `history*` methods of LinkClient.kt).

/**
 * One history row on the wire (`HistoryEntry` of tk-core, camelCase). [watchedAt] is RFC 3339 (`2026-10-27T21:04:11.000Z`).
 * [progressSec] is the exact resume position. Rows without [anilistId] and [malId] are dropped by the PC (E9).
 */
@Serializable
data class LinkHistoryRow(
    val animeId: String,
    val title: String,
    val poster: String? = null,
    val episodeId: String,
    val episode: Int,
    val progressSec: Int = 0,
    val durationSec: Int? = null,
    val completed: Boolean = false,
    val watchedAt: String,
    val anilistId: Int? = null,
    val malId: Int? = null,
)

/**
 * `GET /v1/history?since=<unix s>&limit=<1..=500>`: rows with `watchedAt > since`, OLDEST first. [nextSinceUnix] = `watchedAt` of the
 * last row (or the requested `since` when empty); with [truncated] the phone repeats from [nextSinceUnix]. `since` absent / 0 = everything.
 */
@Serializable
data class LinkHistoryPageDto(
    val items: List<LinkHistoryRow>,
    @SerialName("next_since_unix") val nextSinceUnix: Long,
    val truncated: Boolean = false,
    /** 2026-11: this phone's history mode on the PC (`full` / `receive_only` / `paused`); absent from older PCs. */
    val mode: String? = null,
)

/** `POST /v1/history` body: the phone's dirty rows (<= 500, <= 256 KiB). The PC merges them (newest `watchedAt` wins, tie -> higher progress). */
@Serializable
data class LinkHistoryPushDto(val items: List<LinkHistoryRow>)

/** `POST /v1/history` answer: how many rows entered the PC's store. */
@Serializable
data class LinkHistoryPushResultDto(
    val merged: Int,
    /** 2026-11: the PC does not store this phone's pushes (`receive_only` / `paused`); the rows still count as synced. */
    val ignored: Boolean = false,
    val mode: String? = null,
)

/** `GET /v1/history/mode` (2026-11): this phone's history mode and whether the PC waits for its full push. */
@Serializable
data class LinkHistoryModeDto(val mode: String, val resync: Boolean = false)

/** `POST /v1/history/resync-done` answer: how many other devices the PC asked to replace their history. */
@Serializable
data class LinkResyncDoneDto(val notified: Int = 0)

/** How the paired PC treats this phone's history (tk-link `devices::HistoryMode`). */
enum class LinkHistoryMode(val wire: String) {
    /** Pushes merge and pulls are served (the behaviour before modes existed). */
    FULL("full"),
    /** The phone pulls; what it plays here is not saved on the PC. */
    RECEIVE_ONLY("receive_only"),
    /** Nothing in either direction. */
    PAUSED("paused"),
    ;

    companion object {
        /** null for an absent or unknown value (an older or newer PC). */
        fun fromWire(value: String?): LinkHistoryMode? = entries.firstOrNull { it.wire == value }
    }
}

/** What a `history_resync` event of `GET /v1/events` asks for ("Use this device as the source" on the PC). */
enum class HistoryResyncMode(val wire: String) {
    /** Push every local row, then `POST /v1/history/resync-done`. */
    PUSH_ALL("push_all"),
    /** Push the dirty rows (when the mode is `full`), drop the local history and pull everything from the PC. */
    PULL_REPLACE("pull_replace"),
    ;

    companion object {
        fun fromWire(value: String?): HistoryResyncMode? = entries.firstOrNull { it.wire == value }
    }
}

/** Read side for :feature:link: this phone's history mode as the PC last said it, and whether a resync runs here. */
interface LinkHistoryStatus {
    /** null = unknown (not asked yet, older PC, unpaired). */
    val mode: StateFlow<LinkHistoryMode?>

    /** A `history_resync` is being carried out on this phone. */
    val resyncing: StateFlow<Boolean>

    /** Ask the PC (`GET /v1/history/mode`); failures keep the last value. */
    suspend fun refreshMode()
}

/**
 * Raised by [LinkHistoryRemote] when there is no paired PC (or the pairing is revoked / expired): the history sync treats it as
 * "nothing to do" and the app keeps working local-only. Other [LinkError]s travel as [LinkException] = "retry later".
 */
class LinkHistoryUnavailableException(val reason: LinkError) : Exception("Tatakai Link history unavailable: $reason")

/**
 * The paired PC as a history peer, consumed by `HistorySyncer` (:core:data). Implemented by [LinkHistoryRemoteImpl] over the
 * repository's mTLS [LinkClient]; bound in `linkModule`. Canonical wire shared with desktop contract §3.8 (incl. `DELETE /v1/history`).
 */
interface LinkHistoryRemote {
    /** True while a pairing exists on this phone (the PC may still be unreachable). */
    val available: Boolean

    /** @throws LinkHistoryUnavailableException (not paired / revoked / expired), LinkException (other failures). */
    suspend fun push(rows: List<LinkHistoryRow>): LinkHistoryPushResultDto

    /** @throws LinkHistoryUnavailableException, LinkException. */
    suspend fun since(sinceUnix: Long, limit: Int = LinkClient.MAX_HISTORY_ROWS): LinkHistoryPageDto

    /** `DELETE /v1/history` (204): the PC drops its whole history. Call only after the user confirmed "also on the PC". @throws LinkHistoryUnavailableException, LinkException. */
    suspend fun clear()

    // --- 2026-11 history per device (defaults: a peer without the feature) ---

    /** `history_resync` events from the PC, buffered until collected (`HistorySyncTriggers`). */
    val resyncRequests: Flow<HistoryResyncMode> get() = emptyFlow()

    /** `GET /v1/history/mode`; null when the PC cannot say (older PC, offline). Never throws. */
    suspend fun mode(): LinkHistoryMode? = null

    /** `POST /v1/history/resync-done` after a `push_all`. @throws LinkHistoryUnavailableException, LinkException. */
    suspend fun resyncDone() {}

    /** The syncer starts / ends carrying out a resync (for [LinkHistoryStatus.resyncing]). */
    fun noteResync(running: Boolean) {}
}

/**
 * [LinkHistoryRemote] over the repository's mTLS connection: the same client certificate and pinned CA as every other Link call
 * ([RemotePlaybackAccess.playbackClient]) and the paired PC's host / port. Not paired, TLS revoked / expired, `device_not_authorized`
 * or a PC without the history route (404, Link older than wave A) -> [LinkHistoryUnavailableException] ("nothing to do"); any other
 * [LinkError] -> [LinkException] ("retry later"). Nothing is logged (rows carry titles).
 */
class LinkHistoryRemoteImpl(private val repository: LinkRepositoryImpl) : LinkHistoryRemote, LinkHistoryStatus {
    override val available: Boolean get() = repository.linkedDevice.value != null

    private val _mode = MutableStateFlow<LinkHistoryMode?>(null)
    override val mode: StateFlow<LinkHistoryMode?> = _mode.asStateFlow()
    private val _resyncing = MutableStateFlow(false)
    override val resyncing: StateFlow<Boolean> = _resyncing.asStateFlow()

    override suspend fun push(rows: List<LinkHistoryRow>): LinkHistoryPushResultDto {
        if (rows.isEmpty()) return LinkHistoryPushResultDto(0)
        var merged = 0
        var ignored = false
        var mode: String? = null
        for (chunk in rows.chunked(LinkClient.MAX_HISTORY_ROWS)) {
            val r = unwrap(client().historyPush(chunk))
            merged += r.merged
            ignored = ignored || r.ignored
            mode = r.mode ?: mode
        }
        noteWireMode(mode)
        return LinkHistoryPushResultDto(merged, ignored, mode)
    }

    override suspend fun since(sinceUnix: Long, limit: Int): LinkHistoryPageDto =
        unwrap(client().historySince(sinceUnix, limit)).also { noteWireMode(it.mode) }

    override suspend fun clear() {
        unwrap(client().historyClear())
    }

    override val resyncRequests: Flow<HistoryResyncMode> get() = repository.historyResyncRequests

    override suspend fun mode(): LinkHistoryMode? {
        val c = runCatching { client() }.getOrNull() ?: return null
        val dto = (c.historyMode() as? LinkResult.Ok)?.value ?: return null
        return LinkHistoryMode.fromWire(dto.mode)?.also { _mode.value = it }
    }

    override suspend fun refreshMode() {
        if (available) mode() else _mode.value = null
    }

    override suspend fun resyncDone() {
        unwrap(client().historyResyncDone())
    }

    override fun noteResync(running: Boolean) {
        _resyncing.value = running
    }

    private fun noteWireMode(wire: String?) {
        LinkHistoryMode.fromWire(wire)?.let { _mode.value = it }
    }

    private fun client(): LinkClient {
        val device = repository.linkedDevice.value ?: throw LinkHistoryUnavailableException(LinkError.NotPaired)
        val http = repository.playbackClient() ?: throw LinkHistoryUnavailableException(LinkError.NotPaired)
        return LinkClient(baseUrl(device.host, device.port), http)
    }

    internal companion object {
        /** `https://host:port/`, with brackets for an IPv6 literal. */
        fun baseUrl(host: String, port: Int): okhttp3.HttpUrl =
            okhttp3.HttpUrl.Builder().scheme("https").host(host.removePrefix("[").removeSuffix("]")).port(port).build()

        /** Maps a client result onto the [LinkHistoryRemote] error contract. */
        fun <T> unwrap(result: LinkResult<T>): T = when (result) {
            is LinkResult.Ok -> result.value
            is LinkResult.Failure -> throw failureOf(result.error)
        }

        fun failureOf(error: LinkError): Exception = when {
            error is LinkError.NotPaired -> LinkHistoryUnavailableException(error)
            error is LinkError.Transport.Tls.Revoked || error is LinkError.Transport.Tls.Expired -> LinkHistoryUnavailableException(error)
            error is LinkError.Api && error.error == LinkApiError.DeviceNotAuthorized -> LinkHistoryUnavailableException(error)
            error is LinkError.Unexpected && error.httpStatus == 404 -> LinkHistoryUnavailableException(error)
            else -> LinkException(error)
        }
    }
}
