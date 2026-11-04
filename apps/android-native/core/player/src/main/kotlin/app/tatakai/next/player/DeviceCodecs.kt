// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.VideoCodec

// android wave (docs/redesign/11 §5.3, 12 §4 d.2): "Compatible with this device" filter of the source picker. HEVC 10-bit needs a
// decoder announcing HEVCProfileMain10, AV1 needs any AV1 decoder (AV1ProfileMain10 for 10-bit). Detection is Android; the rule is pure.

/** What the device's decoders can do for the codecs anime releases use. */
data class CodecCaps(val hevc: Boolean = true, val hevcMain10: Boolean = true, val av1: Boolean = true, val av1Main10: Boolean = true) {
    companion object {
        /** Assume everything plays (previews, tests, detection failure). */
        val ALL = CodecCaps()
    }
}

/**
 * false when the release tags say the video needs a decoder the device lacks: HEVC (any / 10-bit), AV1 (any / 10-bit). Unknown codec or
 * bit depth = compatible (web sources have empty tags).
 */
fun isDeviceCompatible(tags: ReleaseTags, caps: CodecCaps): Boolean {
    val tenBit = tags.bitDepth == 10
    return when (tags.codec) {
        VideoCodec.HEVC -> caps.hevc && (!tenBit || caps.hevcMain10)
        VideoCodec.AV1 -> caps.av1 && (!tenBit || caps.av1Main10)
        else -> true
    }
}

object DeviceCodecs {
    @Volatile
    private var cached: CodecCaps? = null

    /** Decoder capabilities (MediaCodecList, computed once per process; [CodecCaps.ALL] if the query fails). */
    fun capabilities(): CodecCaps = cached ?: detect().also { cached = it }

    private fun detect(): CodecCaps = try {
        val decoders = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { !it.isEncoder }
        fun profiles(mime: String): List<Int>? {
            val infos = decoders.filter { info -> info.supportedTypes.any { it.equals(mime, ignoreCase = true) } }
            if (infos.isEmpty()) return null
            return infos.flatMap { info ->
                runCatching { info.getCapabilitiesForType(mime).profileLevels.map { it.profile } }.getOrDefault(emptyList())
            }
        }
        val hevc = profiles(MIME_HEVC)
        val av1 = profiles(MIME_AV1)
        CodecCaps(
            hevc = hevc != null,
            hevcMain10 = hevc?.any { it == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 || it == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 } == true,
            av1 = av1 != null,
            av1Main10 = av1?.any { it == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10 || it == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10HDR10 } == true,
        )
    } catch (e: Exception) {
        CodecCaps.ALL
    }

    private const val MIME_HEVC = "video/hevc"
    private const val MIME_AV1 = "video/av01"
}
