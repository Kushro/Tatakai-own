package app.tatakai.next.model

import kotlinx.serialization.Serializable

enum class Container { HLS, MP4, DASH }

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
)
