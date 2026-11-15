// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

// BUG-132: which of the PC's Link profiles this phone can play. The PC encodes every profile as H.264 at the profile's output size
// (`output.width` x `output.height`) and frame rate (source fps x the frame-generation multiplier). A profile whose size the phone's
// AVC decoder rejects (the moto g51's `c2.qti.avc.decoder` refuses 2560x1440) makes Media3 fail at decoder init, so the phone hides
// nothing but marks those profiles as unavailable, with the reason. Pure, unit tested; the Android detection is in :core:link.

/** What the phone's H.264 decoder accepts (MediaCodecInfo.VideoCapabilities on the device; fakes in tests). */
interface VideoDecodeLimits {
    fun sizeSupported(width: Int, height: Int): Boolean
    fun sizeAndRateSupported(width: Int, height: Int, fps: Double): Boolean
}

/** Whether this phone can play a Link profile. */
sealed interface ProfileSupport {
    data object Supported : ProfileSupport

    /** The decoder does not take this frame size ("Tu teléfono no reproduce [height]p"). */
    data class TooLarge(val height: Int) : ProfileSupport

    /** The size is fine but not at this frame rate ("Tu teléfono no reproduce [height]p a [fps] fps"). */
    data class TooFast(val height: Int, val fps: Int) : ProfileSupport

    /** The decoder already failed with this profile in this player ("Tu teléfono no pudo reproducir este perfil"). */
    data object Rejected : ProfileSupport
}

/** Frame rate the PC's sources are assumed to have when a profile multiplies it (anime: 23.976, rounded up). */
const val LINK_SOURCE_FPS = 24

/**
 * Output frame rate of [profile]: the LSFG auto target, or [LINK_SOURCE_FPS] x the multiplier (`fg_multiplier`, or read from the
 * stable id: `-x<n>`, `-fg<n>x`, `-auto<fps>`), or [LINK_SOURCE_FPS] without frame generation.
 */
fun linkProfileFps(profile: LinkProfile): Int {
    profile.fgTargetFps?.let { return it }
    profile.fgMultiplier?.let { return LINK_SOURCE_FPS * it }
    AUTO_ID.find(profile.id)?.let { return it.groupValues[1].toInt() }
    MULTIPLIER_ID.find(profile.id)?.let { m -> return LINK_SOURCE_FPS * (m.groupValues[1].ifEmpty { m.groupValues[2] }).toInt() }
    return LINK_SOURCE_FPS
}

private val AUTO_ID = Regex("""-auto(\d{2,3})$""")
private val MULTIPLIER_ID = Regex("""-x(\d)$|-fg(\d)x$""")

/**
 * Whether this phone plays [profile]: [ProfileSupport.Rejected] when its id is in [rejected] (the decoder failed with it already),
 * then the decoder's size and size-at-rate checks. Without [limits] (detection failed, previews) or without an output size,
 * everything counts as supported.
 */
fun linkProfileSupport(profile: LinkProfile, limits: VideoDecodeLimits?, rejected: Set<String> = emptySet()): ProfileSupport {
    if (profile.id in rejected) return ProfileSupport.Rejected
    if (limits == null || profile.outputWidth <= 0 || profile.outputHeight <= 0) return ProfileSupport.Supported
    val w = profile.outputWidth
    val h = profile.outputHeight
    if (!limits.sizeSupported(w, h)) return ProfileSupport.TooLarge(h)
    val fps = linkProfileFps(profile)
    if (!limits.sizeAndRateSupported(w, h, fps.toDouble())) return ProfileSupport.TooFast(h, fps)
    return ProfileSupport.Supported
}

/** The ids of [profiles] this phone cannot play, with why (missing = supported). */
fun unsupportedLinkProfiles(
    profiles: List<LinkProfile>,
    limits: VideoDecodeLimits?,
    rejected: Set<String> = emptySet(),
): Map<String, ProfileSupport> = profiles.mapNotNull { p ->
    linkProfileSupport(p, limits, rejected).takeIf { it != ProfileSupport.Supported }?.let { p.id to it }
}.toMap()

/**
 * The profile to use instead of [wanted] (null = [defaultProfile]) when this phone cannot play it (BUG-132): [wanted] itself when it is playable (or unknown
 * to the PC's list: the PC decides); otherwise the closest playable one, i.e. the same kind (upscale / frame generation) with the
 * largest size and frame rate not above [wanted]'s; otherwise `p-none` (the PC's original video); otherwise [defaultProfile] or the
 * first playable profile. null = nothing in the list is playable.
 */
fun playableLinkProfile(
    wanted: String?,
    profiles: List<LinkProfile>,
    limits: VideoDecodeLimits?,
    rejected: Set<String> = emptySet(),
    defaultProfile: String? = null,
): String? {
    val target = wanted ?: defaultProfile
    val usable = profiles.filter { linkProfileSupport(it, limits, rejected) == ProfileSupport.Supported }
    val from = profiles.firstOrNull { it.id == target }
    if (target != null && from == null && target !in rejected) return target
    if (from != null && usable.any { it.id == from.id }) return from.id
    if (from != null) {
        val fromFps = linkProfileFps(from)
        usable
            .filter {
                it.upscale == from.upscale && it.frameGeneration == from.frameGeneration &&
                    it.outputHeight <= from.outputHeight && linkProfileFps(it) <= fromFps
            }
            .maxWithOrNull(compareBy<LinkProfile>({ it.outputHeight }, { linkProfileFps(it) }))
            ?.let { return it.id }
    }
    return usable.firstOrNull { it.id == NO_ENHANCEMENT_PROFILE }?.id
        ?: usable.firstOrNull { it.id == defaultProfile }?.id
        ?: usable.firstOrNull()?.id
}

/** BUG-132: the saved preferred profile [from] is not playable on this phone and [to] is used (and saved) instead. */
data class LinkProfileFallback(val from: String, val to: String)

/** tk-link's profile without enhancement (the PC's video as is). */
const val NO_ENHANCEMENT_PROFILE = "p-none"
