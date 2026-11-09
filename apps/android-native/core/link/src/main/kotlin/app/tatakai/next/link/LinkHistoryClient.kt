// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave A contract, 2026-11-09 amendment (plan 14 §0.1 #9): the watch history (exact resume position) syncs between the user's
// own devices over Tatakai Link instead of Supabase. The PC (tk-link, desktop worker f; desktop contract §3.8) serves `/v1/history`
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
)

/** `POST /v1/history` body: the phone's dirty rows (<= 500, <= 256 KiB). The PC merges them (newest `watchedAt` wins, tie -> higher progress). */
@Serializable
data class LinkHistoryPushDto(val items: List<LinkHistoryRow>)

/** `POST /v1/history` answer: how many rows entered the PC's store. */
@Serializable
data class LinkHistoryPushResultDto(val merged: Int)

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
}

/**
 * Contract stub (worker q): every call goes through the repository's connection (`LinkRepositoryImpl.call`-style: not paired ->
 * [LinkHistoryUnavailableException]; TLS revoked / expired -> [LinkHistoryUnavailableException]; anything else -> [LinkException]).
 * Marks the PC reachable / unreachable like the other calls do.
 */
class LinkHistoryRemoteImpl(private val repository: LinkRepositoryImpl) : LinkHistoryRemote {
    override val available: Boolean get() = repository.linkedDevice.value != null

    override suspend fun push(rows: List<LinkHistoryRow>): LinkHistoryPushResultDto = TODO("worker q")

    override suspend fun since(sinceUnix: Long, limit: Int): LinkHistoryPageDto = TODO("worker q")

    override suspend fun clear(): Unit = TODO("worker q")
}
