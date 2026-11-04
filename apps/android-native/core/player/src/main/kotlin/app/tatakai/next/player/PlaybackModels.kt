// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Container
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.SubtitleTrack
import okhttp3.OkHttpClient
import java.time.Instant

/** One thing the player can play, with the HTTP headers its requests must carry (no local proxy exists natively). */
class PlayableSource(
    val url: String,
    val headers: Map<String, String>,
    val container: Container,
    val label: String,
    val quality: String? = null,
    val subtitles: List<SubtitleTrack> = emptyList(),
    /** Overrides the shared client: used for the PC's HLS, which needs mTLS with the pinned CA. */
    val httpClient: OkHttpClient? = null,
    /** android wave: a torrent loopback stream (bigger buffer, long read timeouts; 11 §5.3). */
    val torrent: Boolean = false,
) {
    companion object {
        fun from(s: StreamSource): PlayableSource =
            PlayableSource(
                url = s.url,
                headers = s.headers,
                container = s.container,
                label = listOfNotNull(s.provider, s.quality).joinToString(" - "),
                quality = s.quality,
                subtitles = s.subtitles,
            )
    }
}

/**
 * One subtitle track. [groupIndex] is the engine's id (Media3 track group index, mpv `sid`). [ass] = SSA/ASS styling
 * (Media3 renders it simplified: the player shows the "simplified subtitles" notice).
 */
data class SubtitleOption(val groupIndex: Int, val label: String, val language: String?, val selected: Boolean, val ass: Boolean = false)

/** One audio track ([id] = Media3 track group index, mpv `aid`). */
data class AudioOption(val id: Int, val label: String, val language: String?, val selected: Boolean)

data class PlaybackError(val code: Int, val name: String, val exhausted: Boolean)

/** Everything the UI renders; produced by [PlaybackController] on the main thread. */
data class PlaybackState(
    val isPlaying: Boolean = false,
    val isBuffering: Boolean = true,
    val ended: Boolean = false,
    val positionMs: Long = 0,
    val durationMs: Long = 0,
    val bufferedMs: Long = 0,
    val speed: Float = 1f,
    val sourceIndex: Int = 0,
    val sourceCount: Int = 0,
    /** Set while falling back automatically to the next source. */
    val fallbackCount: Int = 0,
    val videoHeights: List<Int> = emptyList(),
    /** Null = automatic (adaptive). */
    val selectedHeight: Int? = null,
    val subtitles: List<SubtitleOption> = emptyList(),
    val subtitlesEnabled: Boolean = false,
    val error: PlaybackError? = null,
    // --- android wave (worker d) ---
    /** Bumped by every [PlayerSession.load]; lets callers ignore errors / starts of an older load. */
    val loadId: Int = 0,
    /** The current load produced its first frame / became ready at least once. */
    val started: Boolean = false,
    val audioTracks: List<AudioOption> = emptyList(),
    /** The selected subtitle track is SSA/ASS. */
    val subtitleIsAss: Boolean = false,
    /** The engine renders ASS with full styling (mpv); false for Media3 (simplified). */
    val fullAss: Boolean = false,
)

/**
 * Automatic fallback to the next source: at most [maxFallbacks] (3) automatic moves after failures; a successful start of playback
 * resets the budget. A manual selection also resets it. Pure logic, no Android.
 */
class SourceFallback(private val sourceCount: Int, private val maxFallbacks: Int = 3, start: Int = 0) {
    var index: Int = start
        private set
    var fallbacksUsed: Int = 0
        private set

    /** The index to try after a failure of the current source, or null when the budget or the sources are exhausted. */
    fun onFailure(): Int? {
        if (fallbacksUsed >= maxFallbacks || sourceCount <= 1) return null
        fallbacksUsed++
        index = (index + 1) % sourceCount
        return index
    }

    fun onPlaybackStarted() {
        fallbacksUsed = 0
    }

    fun select(i: Int) {
        require(i in 0 until sourceCount)
        index = i
        fallbacksUsed = 0
    }
}

/** Builds history rows with the E9 keys (`HistoryEntry.animeIdOf` / `episodeIdOf`, completed at >= 0.9). */
object ProgressTracker {
    fun entry(
        anilistId: Int?,
        malId: Int?,
        title: String,
        poster: String?,
        episode: Int,
        positionMs: Long,
        durationMs: Long,
        now: Instant,
    ): HistoryEntry? {
        val animeId = HistoryEntry.animeIdOf(anilistId, malId) ?: return null
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
            anilistId = anilistId?.takeIf { it > 0 },
            malId = malId?.takeIf { it > 0 },
        )
    }
}
