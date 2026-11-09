// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.torrent.DebridRepository

// android wave (docs/redesign/12 §4 d.2): source selection flow of the player, ported 1:1 from desktop
// apps/desktop-native/crates/tk-ui/src/player_state.rs (FlowState). Pure: no Android, no clock, no network.
//
// The view-model drives it like this: after every transition it asks [SourceFlow.toResolve]; non-null means "resolve this source now",
// tagged with [SourceFlow.attempt] so a late answer for an older attempt is ignored. Nothing else in here starts network work, so in
// Manual mode nothing is resolved until [SourceFlow.pick].

enum class FlowPhase {
    /** Resolving `sources[current]` (web: its stream; torrent: `TorrentRepository.play`). */
    RESOLVING,

    /** The stream is loaded into the engine and has not produced a frame yet (or stalls while playing). */
    BUFFERING,
    PLAYING,

    /** Nothing left to try ([SourceFlow.failed] lists every attempt). */
    ERROR,

    /** Manual mode without a pick: waiting for the user, nothing is resolved. */
    MANUAL,
}

/** Why a source failed (`failed[]` of the final ErrorState). */
enum class FailKind { RESOLVE, PLAYBACK, NO_PEERS, NO_FILE, METERED_DECLINED, UNKNOWN }

/** One entry of `failed[]`: [name] = `Ext · Server/Group`, [detail] = technical text (error code / message). */
data class FailedSource(val key: String, val name: String, val kind: FailKind, val detail: String)

/** `Ext · Server` / `Ext · Group`, as in the picker rows. */
fun sourceName(s: RankedSource): String = "${s.candidate.extName} · ${s.candidate.label}"

/** What Auto mode may try on its own: any source of an enabled extension (torrents included since the torrent wave). */
fun playable(s: RankedSource): Boolean = s.extEnabled

class SourceFlow(sources: List<RankedSource>, val mode: SourceMode, private val legacy: Boolean = false) {
    var sources: List<RankedSource> = sources
        private set
    var phase: FlowPhase = FlowPhase.RESOLVING
        private set

    /** Index into [sources] of the source being resolved / played. */
    var current: Int? = null
        private set
    private val _failed = mutableListOf<FailedSource>()
    val failed: List<FailedSource> get() = _failed.toList()
    var errorKind: FailKind? = null
        private set

    /** Bumped every time a source starts resolving. */
    var attempt: Long = 0
        private set
    private var started = false

    init {
        begin()
    }

    private fun begin() {
        _failed.clear()
        errorKind = null
        current = null
        if (sources.isEmpty()) {
            stop(FailKind.RESOLVE)
            return
        }
        val pickedIndex = sources.indexOfFirst { it.picked }
        if (pickedIndex >= 0) {
            enter(pickedIndex)
            return
        }
        if (mode == SourceMode.MANUAL && !legacy) {
            phase = FlowPhase.MANUAL
            return
        }
        val next = nextPlayable()
        if (next != null) enter(next) else stop(FailKind.RESOLVE)
    }

    private fun enter(i: Int) {
        current = i
        phase = FlowPhase.RESOLVING
        attempt++
        started = false
        errorKind = null
    }

    private fun stop(kind: FailKind) {
        phase = FlowPhase.ERROR
        errorKind = kind
    }

    /** First playable source in ranking order that has not failed yet. */
    private fun nextPlayable(): Int? = sources.indices.firstOrNull { i ->
        val s = sources[i]
        playable(s) && _failed.none { it.key == s.key }
    }

    /** Non-null while that source must be resolved; null in every other phase. */
    fun toResolve(): RankedSource? = if (phase == FlowPhase.RESOLVING) currentSource() else null

    fun currentSource(): RankedSource? = current?.let { sources.getOrNull(it) }

    /** `(n, total)` for "Source n of total". */
    fun position(): Pair<Int, Int>? = current?.let { it + 1 to sources.size }

    /** The user picked [key] (picker, "Use the best web source"): marks it as the pick and resolves it, whatever the mode. */
    fun pick(key: String): Boolean {
        val i = sources.indexOfFirst { it.key == key }
        if (i < 0) return false
        sources = sources.map { it.copy(picked = it.key == key) }
        _failed.removeAll { it.key == key }
        enter(i)
        return true
    }

    /** The stream of [attempt] was handed to the engine. */
    fun resolved(attempt: Long) {
        if (attempt == this.attempt && phase == FlowPhase.RESOLVING) phase = FlowPhase.BUFFERING
    }

    /** The engine shows frames. Returns the source key to report as working, once per attempt (never in legacy). */
    fun started(): String? {
        if (phase != FlowPhase.BUFFERING && phase != FlowPhase.PLAYING) return null
        phase = FlowPhase.PLAYING
        if (started) return null
        started = true
        return if (legacy) null else currentSource()?.key
    }

    /** Cache starvation while playing. */
    fun stalled(stalled: Boolean) {
        if (stalled && phase == FlowPhase.PLAYING) {
            phase = FlowPhase.BUFFERING
        } else if (!stalled && phase == FlowPhase.BUFFERING && started) {
            phase = FlowPhase.PLAYING
        }
    }

    /**
     * The source of [attempt] failed (resolve, load or engine error). Records it in `failed`; Auto (and legacy) moves on to the next
     * playable source, Manual stops so the user picks again. Returns the failed key to report (null for stale attempts and legacy).
     */
    fun fail(attempt: Long, kind: FailKind, detail: String): String? {
        if (attempt != this.attempt || phase !in ACTIVE) return null
        val s = currentSource() ?: return null
        _failed += FailedSource(s.key, sourceName(s), kind, detail)
        val report = if (legacy) null else s.key
        if (mode == SourceMode.AUTO || legacy) {
            val n = nextPlayable()
            if (n != null) {
                enter(n)
                return report
            }
        }
        stop(kind)
        return report
    }

    /** "Retry" on the final error: forget the failures and start over (the pick still wins). */
    fun retry() = begin()

    /** Best web source to offer when a torrent fails / on mobile data (not failed yet if possible). */
    fun bestWeb(): RankedSource? {
        val web = sources.filter { playable(it) && it.candidate.kind == SourceKind.WEB }
        return web.firstOrNull { s -> _failed.none { it.key == s.key } } ?: web.firstOrNull()
    }

    /** The last failure was a torrent source (the error screen then offers the best web source). */
    fun lastFailedTorrent(): Boolean {
        val last = _failed.lastOrNull() ?: return false
        return sources.firstOrNull { it.key == last.key }?.candidate?.kind == SourceKind.TORRENT
    }

    private companion object {
        val ACTIVE = setOf(FlowPhase.RESOLVING, FlowPhase.BUFFERING, FlowPhase.PLAYING)
    }
}

/**
 * The ranked list the player works on: [requestedKey] (PlayerArgs.sourceKey from Details) or else [storedPick]
 * (SourceStatsStore.pickedSource) becomes the only `picked` row; the order never changes.
 */
fun applyPick(sources: List<RankedSource>, requestedKey: String?, storedPick: String?): List<RankedSource> {
    val key = requestedKey?.takeIf { k -> sources.any { it.key == k } }
        ?: storedPick?.takeIf { k -> sources.any { it.key == k } }
    if (key != null) return sources.map { it.copy(picked = it.key == key) }
    // No usable pick: keep the repository's flag only when it marks exactly one row.
    return if (sources.count { it.picked } <= 1) sources else sources.map { it.copy(picked = false) }
}

/**
 * parity wave A (15 §3.9): torrent candidates as seen by the linked debrid services. `DebridRepository.markCached` (worker q) fills
 * `SourceCandidate.debrid`; its default is the identity, so without a debrid account (or without the repository bound) nothing changes.
 * A failure of the lookup never blocks playback: the list is returned as it came.
 */
suspend fun markDebrid(sources: List<RankedSource>, debrid: DebridRepository?): List<RankedSource> {
    if (debrid == null || sources.none { it.candidate.kind == SourceKind.TORRENT }) return sources
    val marked = try {
        debrid.markCached(sources.map { it.candidate })
    } catch (e: kotlinx.coroutines.CancellationException) {
        throw e
    } catch (e: Exception) {
        return sources
    }
    if (marked.size != sources.size) return sources
    return sources.mapIndexed { i, s -> s.copy(candidate = marked[i]) }
}
