// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Player-side contract of the android wave (docs/redesign/12 §3.2). Signatures are FINAL.

/**
 * Settings › Reproductor › Motor. [MEDIA3] = ExoPlayer (default); [MPV] = libmpv (dev.jdtech.mpv:libmpv) with OUR controls, full ASS;
 * [EXTERNAL] = ACTION_VIEW to mpv-android / VLC (headers as extras) + informative screen with Stop / Back (desktop external mpv mode).
 * No `EnhancedUi` on Android: the desktop "embedded enhanced + uosc" mode has no Android counterpart in this wave.
 */
@Serializable
enum class PlayerEngine {
    @SerialName("media3") MEDIA3,
    @SerialName("mpv") MPV,
    @SerialName("external") EXTERNAL,
}

/** Subtitle look (Settings › Reproductor › Estilo de subtítulos). Applied by Media3 (CaptionStyle) and mpv (sub-* options; ASS keeps its own styles). */
@Serializable
data class SubtitleStyle(
    /** 0.5..2.0, 1.0 = 100 %. */
    val scale: Float = 1f,
    /** Background box opacity 0..1 (prototype: 60 %). */
    val backgroundOpacity: Float = 0.6f,
    /** "outfit" | "space_grotesk" | "system". */
    val font: String = "outfit",
    val bold: Boolean = false,
    val outline: Boolean = true,
)

/** Intro / outro / recap interval (AniSkip), desktop `SkipKindVm`. Label keys `player_skip_<id>` live in :core:ui. */
@Serializable
enum class SkipKind(val id: String) {
    @SerialName("intro") INTRO("intro"),
    @SerialName("outro") OUTRO("outro"),
    @SerialName("recap") RECAP("recap"),
}

@Serializable
data class SkipMark(val kind: SkipKind, val startSecs: Double, val endSecs: Double)

/**
 * AniSkip lookup by MAL id (`https://api.aniskip.com/v2/skip-times/<mal>/<ep>?types=op&types=ed&types=recap&episodeLength=0`), short timeout,
 * cached per episode, empty without MAL id or data. Used for web AND torrent playback. Implemented by worker d in `:core:player`
 * (bound in `playerModule`). (11 §2.2 had `skipMarks` inside TorrentRepository; it moved here because web playback uses it too.)
 */
interface SkipMarksRepository {
    suspend fun skipMarks(media: MediaRef, episode: Int): List<SkipMark>
}
