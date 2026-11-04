package app.tatakai.next.model

import kotlinx.serialization.Serializable

enum class Container {
    HLS,
    MP4,
    DASH,

    /** android wave (12 §3.2): progressive HTTP file without manifest (torrent loopback MKV/MP4/WebM); the player infers the extractor. */
    PROGRESSIVE,
}

@Serializable
data class SubtitleTrack(val url: String, val lang: String, val label: String, val format: String)

@Serializable
data class StreamSource(
    val url: String,
    val headers: Map<String, String>,
    val container: Container,
    val quality: String?,
    val provider: String,
    val extensionId: String,
    val subtitles: List<SubtitleTrack> = emptyList(),
)

@Serializable
data class PlayerArgs(
    val media: MediaRef,
    val title: String,
    val episode: Int,
    val startMs: Long = 0,
    val remoteSessionId: String? = null,
    // --- android wave (12 §3.2), all optional ---
    /** `sourceKey` chosen in Details (split button / picker): the player tries it first (Manual mode: only it). */
    val sourceKey: String? = null,
    /** Play a file of a torrent already in the session (client "Play this file", batch next episode, completed download). */
    val torrentId: String? = null,
    /** With [torrentId]: the file to play; null = `pickEpisodeFile`. */
    val fileIndex: Int? = null,
    /** A completed download (file path or `content://`): play offline, no network, no source resolution. */
    val localUri: String? = null,
)
