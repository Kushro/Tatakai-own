// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.os.Build
import app.tatakai.next.model.VideoDecodeLimits
import java.util.Locale

/**
 * BUG-132: the phone's H.264 decoders, for the Link profiles ([app.tatakai.next.model.linkProfileSupport]). Media3 plays the PC's
 * stream with a hardware AVC decoder (`c2.qti.avc.decoder` on the moto g51, which refuses 2560x1440), so only hardware decoders
 * count; when the phone reports none, every AVC decoder does. A size / rate is supported when some decoder takes it.
 */
object AvcDecodeLimits {
    @Volatile
    private var cached: Result<VideoDecodeLimits?>? = null

    /** The device's limits (computed once per process), or null when MediaCodecList cannot be read (then nothing is filtered). */
    fun device(): VideoDecodeLimits? = (cached ?: runCatching { detect() }.also { cached = it }).getOrNull()

    private fun detect(): VideoDecodeLimits? {
        val avc = MediaCodecList(MediaCodecList.REGULAR_CODECS).codecInfos.filter { info ->
            !info.isEncoder && info.supportedTypes.any { it.equals(MIME_AVC, ignoreCase = true) }
        }
        val chosen = avc.filter(::isHardware).ifEmpty { avc }
        val caps = chosen.mapNotNull { runCatching { it.getCapabilitiesForType(MIME_AVC).videoCapabilities }.getOrNull() }
        if (caps.isEmpty()) return null
        return object : VideoDecodeLimits {
            override fun sizeSupported(width: Int, height: Int) = caps.any { runCatching { it.isSizeSupported(width, height) }.getOrDefault(false) }
            override fun sizeAndRateSupported(width: Int, height: Int, fps: Double) =
                caps.any { runCatching { it.areSizeAndRateSupported(width, height, fps) }.getOrDefault(false) }
        }
    }

    private fun isHardware(info: MediaCodecInfo): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            info.isHardwareAccelerated
        } else {
            val n = info.name.lowercase(Locale.ROOT)
            !(n.startsWith("omx.google.") || n.startsWith("c2.android.") || n.contains(".sw.") || n.endsWith(".sw"))
        }

    private const val MIME_AVC = "video/avc"
}
