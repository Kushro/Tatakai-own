// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// Tatakai Next 0.4.0 (worker C): where the video is processed (phone, PC over Tatakai Link, or both) and what the PC does.
// The rules of the "Mixto" location (the PC upscales at the source cadence with a profile without frame generation and the
// phone generates the frames) follow Enigmata's Temple (`TempleMixedMode.kt`, AGPL-3.0-or-later, same author), adapted to
// tk-link's fixed profile catalog: tk-link has no per-session tuning, so "fg: none" means an upscale-only profile.

/** Where the player's video is processed. */
enum class ProcessingLocation {
    /** The original source plays on the phone, with the phone's own enhancements (shaders, frame generation). */
    PHONE,

    /** A Tatakai Link session: the PC processes (its profile) and the phone only decodes its H.264 stream. */
    PC,

    /** A Link session with an upscale-only profile (the PC scales at the source cadence) + the phone's frame generation. */
    MIXED,
}

/** `GET /v1/sessions/{id}` as the phone reads it (0.4.0 fields are null on older PCs). */
data class LinkSessionStatus(
    /** `preparing` | `ready` | `failed` | `closed`. */
    val state: String,
    val profileId: String,
    /** The PC's engine is ahead and waits for the player (back-pressure): it keeps up with this profile. */
    val throttled: Boolean = false,
    /** Seconds the PC has ready past the playhead. */
    val leadSecs: Double? = null,
    /** The PC lowered the profile by itself (contract `degraded_from`). */
    val degradedFrom: String? = null,
    val outputWidth: Int? = null,
    val outputHeight: Int? = null,
    /** `none` | `anime4k` (0.4.0). */
    val upscale: String? = null,
    /** Anime4K `a` | `b` | `c` and `s` | `m` | `l` (0.4.0, only with Anime4K). */
    val anime4kMode: String? = null,
    val anime4kVariant: String? = null,
    /** `none` | `frc_amf` | `lsfg` (0.4.0). */
    val fg: String? = null,
    val fgMultiplier: Int? = null,
    val sourceFps: Double? = null,
    val outFps: Double? = null,
    val encoder: String? = null,
    val videoKbps: Int? = null,
    val error: String? = null,
    /** 0.4.0: the source's subtitle tracks the PC serves (`subtitles`); null = not listed (yet) or an older PC. */
    val subtitles: List<LinkSubtitleTrack>? = null,
    /** 0.4.0: `listing` | `unavailable` | `idle` | `extracting` | `ready` | `failed`; null = an older PC. */
    val subtitlesState: String? = null,
)

/**
 * 0.4.0: one subtitle track of a Link session's source (`GET /v1/sessions/{id}` `subtitles`), served by the PC as
 * `subtitles/{index}.ass` (ASS / SSA, styles kept: [formats] has `ass`) and `.vtt` (any text track). Image tracks (PGS,
 * VobSub) have [text] false and are never served. The files keep the source's own timeline: no offset on the phone.
 */
data class LinkSubtitleTrack(
    val index: Int,
    val language: String? = null,
    val title: String? = null,
    val codec: String = "",
    val default: Boolean = false,
    val forced: Boolean = false,
    val text: Boolean = true,
    val formats: List<String> = emptyList(),
) {
    val hasAss: Boolean get() = FORMAT_ASS in formats
    val hasVtt: Boolean get() = FORMAT_VTT in formats

    companion object {
        const val FORMAT_ASS = "ass"
        const val FORMAT_VTT = "vtt"
    }
}

/** The PC's upscaling in [LinkProcessingInfo]. */
sealed interface LinkUpscale {
    data object None : LinkUpscale

    /** Anime4K; [mode] (`a` / `b` / `c`) and [variant] (`s` / `m` / `l`) are null when the PC does not say (older PC). */
    data class Anime4k(val mode: String? = null, val variant: String? = null) : LinkUpscale
}

/** The PC's frame generation in [LinkProcessingInfo]. */
sealed interface LinkFrameGen {
    data object None : LinkFrameGen

    /** LSFG with a fixed [multiplier]; [outFps] once the PC reports it. */
    data class Lsfg(val multiplier: Int?, val outFps: Double? = null) : LinkFrameGen

    /**
     * LSFG "auto": the PC picks the multiplier per source to approach [targetFps]; [multiplier] / [outFps] = what it chose
     * for this video (0.4.0 `fg_effective`; null on an older PC, which only says "auto").
     */
    data class LsfgAuto(val targetFps: Int?, val multiplier: Int? = null, val outFps: Double? = null) : LinkFrameGen

    /** AMD FRC (Windows): always doubles. */
    data object FrcAmf : LinkFrameGen
}

/** Everything the player says about what the PC does with the playing session ("En la PC: Anime4K A + LSFG ×5 → 120 fps"). */
data class LinkProcessingInfo(
    /** The paired PC's name. */
    val pcName: String?,
    val profileId: String,
    /** The PC's label of the profile (English; the screen names it with `LinkProfileName`). */
    val profileLabel: String?,
    val upscale: LinkUpscale,
    val outputWidth: Int?,
    val outputHeight: Int?,
    val frameGen: LinkFrameGen,
    val encoder: String?,
    val videoKbps: Int?,
    /** The PC is ahead and waits for the player (it keeps up). */
    val throttled: Boolean = false,
    /** The PC lowered the profile by itself: from this one. */
    val degradedFrom: String? = null,
    val leadSecs: Double? = null,
    /** `preparing` | `ready` | `failed` | `closed`; null = the PC was not asked yet. */
    val sessionState: String? = null,
    /** Capabilities `measured_sustain` of the profile: `true` | `false` | `unknown`. */
    val measuredSustain: String = "unknown",
) {
    /** Playing, not held by back-pressure and less than [LOW_LEAD_SECS] ready: the PC may not keep up with this profile. */
    val fallingBehind: Boolean
        get() = sessionState == "ready" && !throttled && (leadSecs ?: Double.MAX_VALUE) < LOW_LEAD_SECS

    companion object {
        const val LOW_LEAD_SECS = 4.0
    }
}

/**
 * What the PC does for the session played with [profileId]: the capabilities' [profile] (may be null: unknown to the list)
 * completed with the session [status] (null before the first poll, or an older PC without the 0.4.0 fields).
 */
fun linkProcessingInfo(pcName: String?, profileId: String, profile: LinkProfile?, status: LinkSessionStatus?): LinkProcessingInfo {
    val id = status?.profileId?.takeIf { it.isNotBlank() } ?: profileId
    val p = profile?.takeIf { it.id == id }
    val upscale = when {
        status?.upscale == "anime4k" -> LinkUpscale.Anime4k(status.anime4kMode, status.anime4kVariant)
        status?.upscale == "none" -> LinkUpscale.None
        p?.upscale == true -> LinkUpscale.Anime4k()
        else -> LinkUpscale.None
    }
    val fgKind = status?.fg ?: p?.fgKind ?: if (p?.frameGeneration == true) "lsfg" else "none"
    val frameGen = when (fgKind) {
        "frc_amf" -> LinkFrameGen.FrcAmf
        "lsfg" -> {
            val target = p?.fgTargetFps ?: AUTO_TARGET.find(id)?.groupValues?.get(1)?.toIntOrNull()
            if (target != null && p?.fgMultiplier == null) {
                LinkFrameGen.LsfgAuto(target, status?.fgMultiplier, status?.outFps)
            } else {
                LinkFrameGen.Lsfg(status?.fgMultiplier ?: p?.fgMultiplier ?: FIXED_MULTIPLIER.find(id)?.groupValues?.get(1)?.toIntOrNull(), status?.outFps)
            }
        }
        else -> LinkFrameGen.None
    }
    return LinkProcessingInfo(
        pcName = pcName,
        profileId = id,
        profileLabel = p?.label,
        upscale = upscale,
        outputWidth = status?.outputWidth ?: p?.outputWidth?.takeIf { it > 0 },
        outputHeight = status?.outputHeight ?: p?.outputHeight?.takeIf { it > 0 },
        frameGen = frameGen,
        encoder = status?.encoder ?: p?.encoder?.takeIf { it.isNotBlank() },
        videoKbps = status?.videoKbps,
        throttled = status?.throttled ?: false,
        degradedFrom = status?.degradedFrom,
        leadSecs = status?.leadSecs,
        sessionState = status?.state,
        measuredSustain = p?.measuredSustain ?: "unknown",
    )
}

private val AUTO_TARGET = Regex("""-auto(\d{2,3})$""")
private val FIXED_MULTIPLIER = Regex("""-x(\d)$""")

// ---- bindings for the other 0.4.0 workers (no-op until the coordinator binds them) -------------------------------------------

/**
 * Worker A's quality engine (local shaders / upscalers on MPV). While the PC processes ([ProcessingLocation.PC] and
 * [ProcessingLocation.MIXED]) the player calls [setRemoteProcessing] with true: the phone must not run its own shaders on
 * the PC's already processed stream; false when it plays the original source again. The coordinator binds it to A's
 * quality API (`remoteProcessing` flag) in Koin: `single<LocalEnhancementGate> { ... }`.
 */
fun interface LocalEnhancementGate {
    fun setRemoteProcessing(on: Boolean)

    companion object {
        /** Nothing bound: the call does nothing. */
        val NONE: LocalEnhancementGate = LocalEnhancementGate { }
    }
}

/** Whether the phone can generate frames (worker B's eligibility), and why not. */
data class PhoneFrameGenAvailability(
    val eligible: Boolean,
    /** Worker B's reason id when not eligible (e.g. `vulkan`, `library_missing`); the UI maps it. null = unknown / eligible. */
    val reason: String? = null,
) {
    companion object {
        val UNBOUND = PhoneFrameGenAvailability(eligible = false, reason = REASON_NOT_AVAILABLE)

        /** Nothing bound: phone frame generation does not exist in this build. */
        const val REASON_NOT_AVAILABLE = "not_available"
    }
}

/**
 * Worker B's phone frame generation as the Link modes need it. [availability] decides whether "Mixto" can be chosen;
 * [setProcessingLocation] is called on every load: PHONE = the user's own frame generation preference applies; PC = off
 * (the phone processes nothing in strict "PC"); MIXED = on, over the PC's stream. The coordinator binds it to B's
 * `FrameGenController` in Koin: `single<PhoneFrameGenGate> { ... }`.
 */
interface PhoneFrameGenGate {
    val availability: StateFlow<PhoneFrameGenAvailability>

    fun setProcessingLocation(location: ProcessingLocation)

    companion object {
        /** Nothing bound: never eligible ("Mixto" stays blocked with [PhoneFrameGenAvailability.REASON_NOT_AVAILABLE]). */
        val NONE: PhoneFrameGenGate = object : PhoneFrameGenGate {
            override val availability: StateFlow<PhoneFrameGenAvailability> =
                MutableStateFlow(PhoneFrameGenAvailability.UNBOUND).asStateFlow()

            override fun setProcessingLocation(location: ProcessingLocation) = Unit
        }
    }
}
