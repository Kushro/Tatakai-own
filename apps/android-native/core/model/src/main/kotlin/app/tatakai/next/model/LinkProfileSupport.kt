// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

// BUG-132: which of the PC's Link profiles this phone can play. The PC encodes every profile as H.264 at the profile's output size
// (`output.width` x `output.height`) and frame rate (source fps x the frame-generation multiplier). A profile whose size the phone's
// AVC decoder rejects (the moto g51's `c2.qti.avc.decoder` refuses 2560x1440) makes Media3 fail at decoder init, so the phone hides
// nothing but marks those profiles as unavailable, with the reason. Pure, unit tested; the Android detection is in :core:link.
// BUG-135: only the frame size blocks a profile. The frame rate the decoder declares (its "performance points") is conservative:
// the g51 declares about 1080p60 yet plays LSFG auto120 fine, so a rate above it is only a note on the row ("may drop frames").

/** What the phone's H.264 decoder accepts (MediaCodecInfo.VideoCapabilities on the device; fakes in tests). */
interface VideoDecodeLimits {
    fun sizeSupported(width: Int, height: Int): Boolean
    /** Only for the "may drop frames" note (BUG-135): never blocks a profile. */
    fun sizeAndRateSupported(width: Int, height: Int, fps: Double): Boolean
}

/** Whether this phone can play a Link profile. */
sealed interface ProfileSupport {
    data object Supported : ProfileSupport

    /** The decoder does not take this frame size ("Tu teléfono no reproduce [height]p"). */
    data class TooLarge(val height: Int) : ProfileSupport

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
 * then the decoder's size check (BUG-135: never the frame rate, see [linkProfileMayDropFrames]). Without [limits] (detection
 * failed, previews) or without an output size, everything counts as supported.
 */
fun linkProfileSupport(profile: LinkProfile, limits: VideoDecodeLimits?, rejected: Set<String> = emptySet()): ProfileSupport {
    if (profile.id in rejected) return ProfileSupport.Rejected
    if (limits == null || profile.outputWidth <= 0 || profile.outputHeight <= 0) return ProfileSupport.Supported
    val w = profile.outputWidth
    val h = profile.outputHeight
    if (!limits.sizeSupported(w, h)) return ProfileSupport.TooLarge(h)
    return ProfileSupport.Supported
}

/**
 * BUG-135: whether [profile]'s output frame rate is above what the phone's decoder declares for its size (the row shows "Puede
 * perder cuadros en este teléfono"; the profile stays selectable). false when the size itself is not supported (that row is
 * disabled with its own reason) or without [limits] / output size.
 */
fun linkProfileMayDropFrames(profile: LinkProfile, limits: VideoDecodeLimits?): Boolean {
    if (limits == null || profile.outputWidth <= 0 || profile.outputHeight <= 0) return false
    val w = profile.outputWidth
    val h = profile.outputHeight
    if (!limits.sizeSupported(w, h)) return false
    return !limits.sizeAndRateSupported(w, h, linkProfileFps(profile).toDouble())
}

/** BUG-135: the ids of [profiles] that this phone plays but may drop frames with ([linkProfileMayDropFrames]). */
fun linkProfilesMayDropFrames(profiles: List<LinkProfile>, limits: VideoDecodeLimits?): Set<String> =
    profiles.filter { linkProfileMayDropFrames(it, limits) }.mapTo(LinkedHashSet()) { it.id }

/** The ids of [profiles] this phone cannot play, with why (missing = supported). */
fun unsupportedLinkProfiles(
    profiles: List<LinkProfile>,
    limits: VideoDecodeLimits?,
    rejected: Set<String> = emptySet(),
): Map<String, ProfileSupport> = profiles.mapNotNull { p ->
    linkProfileSupport(p, limits, rejected).takeIf { it != ProfileSupport.Supported }?.let { p.id to it }
}.toMap()

/**
 * The profile to use instead of [wanted] (null = [defaultProfile]) when this phone cannot play it (BUG-132; BUG-135: only by
 * size, a frame rate above the declared one never replaces it): [wanted] itself when it is playable (or unknown
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

// ---- 0.4.0: the in-player profile list, grouped and filtered like the Linked screen --------------------------------------------

/** Groups of the in-player profile list ("Sin mejora", "Solo escalado", "Solo generación de cuadros", "Escalado + generación"). */
enum class LinkProfileGroupKind { NONE, UPSCALE, FRAME_GEN, UPSCALE_FRAME_GEN }

fun linkProfileGroupKind(profile: LinkProfile): LinkProfileGroupKind = when {
    profile.upscale && profile.frameGeneration -> LinkProfileGroupKind.UPSCALE_FRAME_GEN
    profile.upscale -> LinkProfileGroupKind.UPSCALE
    profile.frameGeneration -> LinkProfileGroupKind.FRAME_GEN
    else -> LinkProfileGroupKind.NONE
}

/** One row: [support] != [ProfileSupport.Supported] = disabled with that reason; [mayDropFrames] = selectable with a note. */
data class LinkProfileRow(val profile: LinkProfile, val support: ProfileSupport, val mayDropFrames: Boolean) {
    val enabled: Boolean get() = support == ProfileSupport.Supported
}

data class LinkProfileGroup(val kind: LinkProfileGroupKind, val rows: List<LinkProfileRow>)

/**
 * The in-player profile list: like the Linked screen, profiles the PC measured as not sustainable (`measured_sustain` false)
 * are hidden, except [current] (the one playing stays visible); profiles this phone cannot decode stay, disabled with the
 * reason (BUG-132); a frame rate above the decoder's declared one is only a note (BUG-135). Grouped by [LinkProfileGroupKind]
 * in that order, the PC's order inside each group; empty groups are left out. [upscaleOnly] keeps only the "Solo escalado"
 * group (and "Sin mejora"): what "Mixto" can use.
 */
fun groupedLinkProfiles(
    profiles: List<LinkProfile>,
    limits: VideoDecodeLimits?,
    rejected: Set<String> = emptySet(),
    current: String? = null,
    upscaleOnly: Boolean = false,
): List<LinkProfileGroup> {
    val visible = profiles.filter { it.measuredSustain != "false" || it.id == current }
        .filter { !upscaleOnly || !it.frameGeneration }
    return LinkProfileGroupKind.entries.mapNotNull { kind ->
        val rows = visible.filter { linkProfileGroupKind(it) == kind }.map {
            LinkProfileRow(it, linkProfileSupport(it, limits, rejected), linkProfileMayDropFrames(it, limits))
        }
        rows.takeIf { it.isNotEmpty() }?.let { LinkProfileGroup(kind, it) }
    }
}

/**
 * 0.4.0 "Mixto": the PC profile to use with the phone's frame generation: an upscale-only profile (no frame generation on the
 * PC: tk-link's way of "fg: none, source cadence") that this phone decodes and the PC did not measure as unsustainable.
 * [wanted] itself when it is one; else the one with the same output height as [wanted] (the upscale part of an
 * "Anime4K + LSFG" profile); else the closest to 1080p (the phone also interpolates: a smaller stream leaves it more room),
 * preferring the smaller one on a tie. null = the PC has none ("Mixto" is blocked).
 */
fun mixedLinkProfile(
    wanted: String?,
    profiles: List<LinkProfile>,
    limits: VideoDecodeLimits?,
    rejected: Set<String> = emptySet(),
): String? {
    val usable = profiles.filter {
        it.upscale && !it.frameGeneration && it.measuredSustain != "false" &&
            linkProfileSupport(it, limits, rejected) == ProfileSupport.Supported
    }
    if (usable.isEmpty()) return null
    usable.firstOrNull { it.id == wanted }?.let { return it.id }
    val from = profiles.firstOrNull { it.id == wanted }
    if (from != null && from.upscale) usable.firstOrNull { it.outputHeight == from.outputHeight }?.let { return it.id }
    return usable.minWithOrNull(compareBy<LinkProfile>({ kotlin.math.abs(it.outputHeight - MIXED_TARGET_HEIGHT) }, { it.outputHeight }))?.id
}

/** "Mixto" aims at this output height when nothing else says which upscale profile to use. */
const val MIXED_TARGET_HEIGHT = 1080
