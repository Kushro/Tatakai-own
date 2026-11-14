// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.VideoCodec

// android wave (docs/redesign/11 §5.3, 12 §4 d.2): "Compatible with this device" filter of the source picker. HEVC 10-bit needs a
// decoder announcing HEVCProfileMain10, AV1 needs any AV1 decoder (AV1ProfileMain10 for 10-bit). Detection is Android; the rule is pure.

/**
 * What the device's decoders can do for the codecs anime releases use. The plain flags count every MediaCodec decoder (software
 * ones included: Media3 can play with them); the `Hw` flags only hardware decoders (BUG-017: without them the video decodes in software
 * and "may be slow on this device").
 */
data class CodecCaps(
    val hevc: Boolean = true,
    val hevcMain10: Boolean = true,
    val av1: Boolean = true,
    val av1Main10: Boolean = true,
    val hevcHw: Boolean = hevc,
    val hevcMain10Hw: Boolean = hevcMain10,
    val av1Hw: Boolean = av1,
    val av1Main10Hw: Boolean = av1Main10,
) {
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

/**
 * true when a hardware decoder handles the video the release tags describe. Unknown codec (web sources) or H.264 = true: every Android
 * device decodes H.264 in hardware, and nothing is known about the rest.
 */
fun decodesInHardware(tags: ReleaseTags, caps: CodecCaps): Boolean {
    val tenBit = tags.bitDepth == 10
    return when (tags.codec) {
        VideoCodec.HEVC -> caps.hevcHw && (!tenBit || caps.hevcMain10Hw)
        VideoCodec.AV1 -> caps.av1Hw && (!tenBit || caps.av1Main10Hw)
        else -> true
    }
}

/**
 * BUG-017 / BUG-020 (plan §11.2): sources this device decodes in hardware keep their ranking order and go first; the ones that need a
 * software decoder (or no decoder at all) follow, in their order. When the recommended row moves down, the first row becomes the
 * recommendation. The `picked` flag is untouched (an explicit choice still plays).
 */
fun rankForDevice(sources: List<RankedSource>, caps: CodecCaps): List<RankedSource> {
    val (fast, slow) = sources.partition { decodesInHardware(it.candidate.tags, caps) }
    if (slow.isEmpty() || fast.isEmpty()) return sources
    val hadRecommendation = sources.any { it.recommended }
    val out = fast + slow
    return if (!hadRecommendation || out.first().recommended) out else out.mapIndexed { i, s -> s.copy(recommended = i == 0) }
}

object DeviceCodecs {
    @Volatile
    private var cached: CodecCaps? = null

    /** Decoder capabilities (MediaCodecList, computed once per process; [CodecCaps.ALL] if the query fails). */
    fun capabilities(): CodecCaps = cached ?: detect().also { cached = it }

    private fun detect(): CodecCaps = try {
        val decoders = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { !it.isEncoder }
        fun profiles(mime: String, hardwareOnly: Boolean): List<Int>? {
            val infos = decoders.filter { info ->
                info.supportedTypes.any { it.equals(mime, ignoreCase = true) } && (!hardwareOnly || isHardware(info))
            }
            if (infos.isEmpty()) return null
            return infos.flatMap { info ->
                runCatching { info.getCapabilitiesForType(mime).profileLevels.map { it.profile } }.getOrDefault(emptyList())
            }
        }
        fun List<Int>?.hevc10() =
            this?.any { it == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10 || it == MediaCodecInfo.CodecProfileLevel.HEVCProfileMain10HDR10 } == true
        fun List<Int>?.av1Ten() =
            this?.any { it == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10 || it == MediaCodecInfo.CodecProfileLevel.AV1ProfileMain10HDR10 } == true
        val hevc = profiles(MIME_HEVC, hardwareOnly = false)
        val av1 = profiles(MIME_AV1, hardwareOnly = false)
        val hevcHw = profiles(MIME_HEVC, hardwareOnly = true)
        val av1Hw = profiles(MIME_AV1, hardwareOnly = true)
        CodecCaps(
            hevc = hevc != null,
            hevcMain10 = hevc.hevc10(),
            av1 = av1 != null,
            av1Main10 = av1.av1Ten(),
            hevcHw = hevcHw != null,
            hevcMain10Hw = hevcHw.hevc10(),
            av1Hw = av1Hw != null,
            av1Main10Hw = av1Hw.av1Ten(),
        )
    } catch (e: Exception) {
        CodecCaps.ALL
    }

    /** API 29+ asks the codec; before that, the platform's software decoders are recognised by name. */
    private fun isHardware(info: MediaCodecInfo): Boolean =
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
            info.isHardwareAccelerated
        } else {
            val n = info.name.lowercase(java.util.Locale.ROOT)
            !(n.startsWith("omx.google.") || n.startsWith("c2.android.") || n.contains(".sw.") || n.endsWith(".sw"))
        }

    private const val MIME_HEVC = "video/hevc"
    private const val MIME_AV1 = "video/av01"
}
