// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.ProviderEntryRef
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.torrent.DebridRepository
import java.time.Instant

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

// ---- parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.3 / §3.4, worker t; additive) -----------------------------

/**
 * The playable stream of a web [candidate] (checklist sources.embed-sniffer): a direct HLS / MP4 candidate is its own stream; an embed
 * page (`SourceCandidate.isEmbed`) is opened by [sniffer] in the hidden WebView with the extension's headers and the user's timeout
 * (`SourcePrefs.embedTimeoutSecs`, 5..60 s) and the captured media request is played with the candidate's identity.
 *
 * Returns null when a non-embed candidate has no playable stream. Throws [EmbedSniffException] when the sniff fails (or no sniffer is
 * bound): the player records it as a RESOLVE failure of that source with [embedFailDetail], reports it and moves to the next source.
 */
suspend fun resolveWebStream(candidate: SourceCandidate, sniffer: EmbedSniffer?, prefs: SourcePrefs): StreamSource? {
    if (candidate.kind != SourceKind.WEB) return null
    if (!candidate.isEmbed) return candidate.toStream()
    val web = candidate.web ?: throw EmbedSniffException(EmbedSniffException.NO_MEDIA)
    if (!prefs.allowEmbed) throw EmbedSniffException(EmbedSniffException.BLOCKED)
    val s = sniffer ?: throw EmbedSniffException(EmbedSniffException.UNAVAILABLE)
    val timeoutMs = prefs.sanitized().embedTimeoutSecs * 1000L
    return s.sniff(web.url, web.headers, candidate.extId, timeoutMs).toStream(candidate)
}

/** Fail detail of an embed source (`failed[]` of the error screen): `embed: timeout` … */
fun embedFailDetail(e: EmbedSniffException): String = "embed: ${e.reason}"

/**
 * checklist ext.custom-providers.watch: the sources of a provider entry ([PlayerArgs.provider]) as the list the [SourceFlow] works on.
 * `ExtensionRepository.providerWatch` already returns them in the extension's order (no ranking between extensions: there is only one),
 * so the order is kept, the first one is `recommended` and labels are made unique (the flow tells sources apart by `sourceKey`).
 */
suspend fun providerSources(extensions: ExtensionRepository, ref: ProviderEntryRef, extName: String? = null): List<RankedSource> {
    val name = extName ?: extensions.installed.value.firstOrNull { it.id == ref.extensionId }?.name ?: ref.extensionId
    return providerRanked(extensions.providerWatch(ref.extensionId, ref.sourceId, ref.itemId, ref.entryId), name)
}

/** Pure part of [providerSources]: order kept, scores descending, duplicate labels numbered (`Server`, `Server 2`, …). */
fun providerRanked(streams: List<StreamSource>, extName: String): List<RankedSource> {
    val seen = HashMap<String, Int>()
    return streams.mapIndexed { i, s ->
        val base = s.provider.ifBlank { extName }
        val n = (seen[base] ?: 0) + 1
        seen[base] = n
        val label = if (n == 1) base else "$base $n"
        val candidate = SourceCandidate.fromStream(s.copy(provider = label), extName)
        RankedSource(candidate = candidate, score = (streams.size - i).toFloat(), extEnabled = true, recommended = i == 0)
    }
}

/**
 * `HistoryEntry.animeId` of what is playing (checklist ext.custom-providers.continue): the AniList / MAL id when the media has one,
 * otherwise `ProviderEntryRef.historyId` (`ext:<ext>:<source>:<item>`) for a provider entry; null = nothing is recorded.
 */
fun historyAnimeId(anilistId: Int?, malId: Int?, provider: ProviderEntryRef?): String? =
    HistoryEntry.animeIdOf(anilistId, malId) ?: provider?.historyId

/**
 * [ProgressTracker.entry] that also records provider titles: with an AniList / MAL id it is exactly [ProgressTracker.entry]; without
 * one, a [provider] entry is recorded under its `historyId` (anilistId / malId null) so "Continue watching" shows it.
 */
fun providerHistoryEntry(
    anilistId: Int?,
    malId: Int?,
    provider: ProviderEntryRef?,
    title: String,
    poster: String?,
    episode: Int,
    positionMs: Long,
    durationMs: Long,
    now: Instant,
): HistoryEntry? {
    ProgressTracker.entry(anilistId, malId, title, poster, episode, positionMs, durationMs, now)?.let { return it }
    val animeId = provider?.historyId ?: return null
    val progress = (positionMs / 1000).toInt().coerceAtLeast(0)
    val duration = (durationMs / 1000).toInt().takeIf { durationMs > 0 && it > 0 }
    return HistoryEntry(
        animeId = animeId,
        title = title,
        poster = poster,
        episodeId = HistoryEntry.episodeIdOf(animeId, episode),
        episode = episode,
        progressSec = progress,
        durationSec = duration,
        completed = HistoryEntry.isCompleted(progress, duration, false),
        watchedAt = now,
        anilistId = null,
        malId = null,
    )
}
