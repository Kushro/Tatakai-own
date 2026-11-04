// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

// Release tags mirrored 1:1 from apps/desktop-native/crates/tk-core/src/release.rs (docs/redesign/10 §3, 12 §3.1).
// The parser (parseRelease / pickEpisodeFile) lives in :core:release. Signatures are FINAL.

/** Where the video comes from. [label] is a technical code, never translated. */
@Serializable
enum class ReleaseSource {
    @SerialName("bd") BD,
    @SerialName("remux") REMUX,
    @SerialName("web") WEB,
    @SerialName("tv") TV,
    @SerialName("dvd") DVD,
    ;

    fun label(): String = when (this) {
        BD -> "BD"
        REMUX -> "REMUX"
        WEB -> "WEB"
        TV -> "TV"
        DVD -> "DVD"
    }
}

@Serializable
enum class VideoCodec {
    @SerialName("h264") H264,
    @SerialName("hevc") HEVC,
    @SerialName("av1") AV1,
    @SerialName("vp9") VP9,
    ;

    fun label(): String = when (this) {
        H264 -> "H.264"
        HEVC -> "HEVC"
        AV1 -> "AV1"
        VP9 -> "VP9"
    }
}

/** Episode or episode range, `(12, 12)` for a single episode. JSON: a two-element array `[first, last]` (Rust tuple). */
@Serializable(with = EpisodeRangeSerializer::class)
data class EpisodeRange(val first: Int, val last: Int) {
    operator fun contains(episode: Int): Boolean = episode in first..last
}

object EpisodeRangeSerializer : KSerializer<EpisodeRange> {
    private val delegate = ListSerializer(Int.serializer())
    override val descriptor: SerialDescriptor = delegate.descriptor
    override fun serialize(encoder: Encoder, value: EpisodeRange) = encoder.encodeSerializableValue(delegate, listOf(value.first, value.last))
    override fun deserialize(decoder: Decoder): EpisodeRange {
        val list = decoder.decodeSerializableValue(delegate)
        require(list.size == 2) { "episodes must be [first, last]" }
        return EpisodeRange(list[0], list[1])
    }
}

/** What a release name says. Every field is null / false when the name does not say it. */
@Serializable
data class ReleaseTags(
    val group: String? = null,
    /** Anime title as written in the name, without group / episode / tags. */
    val title: String? = null,
    val season: Int? = null,
    val episodes: EpisodeRange? = null,
    val source: ReleaseSource? = null,
    val resolution: Quality = Quality.UNKNOWN,
    val codec: VideoCodec? = null,
    /** 10 for `10-bit` / `Hi10P`, 8 when stated. */
    val bitDepth: Int? = null,
    val dualAudio: Boolean = false,
    val multiSub: Boolean = false,
    /** Season pack / complete batch. */
    val batch: Boolean = false,
    /** SeaDex "best" release. Never set from the name: the extension mapping copies it (`seadexBest` / `isBest`). */
    val seadexBest: Boolean = false,
)
