// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.model.LinkProfileGroup
import app.tatakai.next.model.PhoneFrameGenAvailability
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.ProcessingLocation

// Tatakai Next 0.4.0 (worker C): "Procesar en: Teléfono | PC | Mixto" in the player. The state worker D draws
// (PlayerViewModel.processing) and the pure rules of which location can be chosen and why not. Modelled on Enigmata's
// quality sheet / TemplePlayerBridge (AGPL-3.0-or-later, same author), adapted to Tatakai's player and tk-link.

/** Why a processing location cannot be chosen right now (the option is shown disabled with this reason). */
enum class ProcessingBlock {
    /** PC / Mixto: no PC is paired ("Enlazá una PC en Ajustes › Enlace"). */
    NOT_PAIRED,

    /** PC / Mixto: the paired PC does not answer. */
    PC_OFFLINE,

    /** PC / Mixto: a download on the phone: the PC cannot read it. */
    LOCAL_FILE,

    /** PC / Mixto: this source cannot be sent (a torrent of a title without an AniList id, a provider title...). */
    SOURCE_NOT_SENDABLE,

    /** PC / Mixto: nothing plays yet (resolving, picking a source, error). */
    NO_SOURCE_YET,

    /** Any: the external player is in use. */
    EXTERNAL_PLAYER,

    /** Teléfono: a PC video without a title the phone can look up (a PC library file): it can only play from the PC. */
    NO_ORIGINAL_SOURCE,

    /** Mixto: libmpv (needed for the phone's frame generation) is not in this build. */
    MPV_UNAVAILABLE,

    /** Mixto: this phone cannot generate frames ([ProcessingOption.detail] = worker B's reason id). */
    PHONE_FG_UNAVAILABLE,

    /** Mixto: the PC does not allow the phone's frame generation on its stream (`client_fg: suppressed`, an older PC). */
    PC_SUPPRESSES_PHONE_FG,

    /** Mixto: the PC has no upscale-only profile this phone can play. */
    PC_NO_UPSCALE_PROFILE,
}

/** One choice of "Procesar en": available when [block] is null. */
data class ProcessingOption(val location: ProcessingLocation, val block: ProcessingBlock? = null, val detail: String? = null) {
    val available: Boolean get() = block == null
}

/** How the last switch of location ended (a notice over the video). */
enum class ProcessingFailure {
    /** The PC did not answer in time. */
    TIMEOUT,

    /** No connection with the PC. */
    OFFLINE,

    /** The PC refused this source (it cannot open the link / torrent). */
    SOURCE_REFUSED,

    /** The PC answered with another error, or the stream could not be opened. */
    PC_ERROR,

    /** The phone's playback could not be resumed (nothing to play locally). */
    LOCAL_UNAVAILABLE,
}

/**
 * What the player shows about where the video is processed (worker D draws it; [PlayerViewModel.processing]).
 *
 * - [location]: where it is processed now; [options]: the three choices with their block reasons;
 * - [switchingTo]: a switch is running (the video keeps playing where it was until the new one is ready);
 * - [pc]: what the PC does (PC / Mixto only), refreshed every few seconds from `GET /v1/sessions/{id}`;
 * - [profileGroups]: the PC profiles for the in-player list ("Solo escalado", "Solo generación de cuadros",
 *   "Escalado + generación"; only "Solo escalado" in Mixto), [profile] the one in use, [profileChanging] a change runs;
 * - [phoneFrameGen]: the phone generates frames (Mixto); [phoneFrameGenAvailability]: worker B's eligibility;
 * - [localEnhancementsOff]: the phone's shaders are suspended because the PC processes (worker A's `remoteProcessing`).
 */
data class ProcessingUiState(
    val location: ProcessingLocation = ProcessingLocation.PHONE,
    val options: List<ProcessingOption> = ProcessingLocation.entries.map { ProcessingOption(it) },
    val switchingTo: ProcessingLocation? = null,
    val pc: LinkProcessingInfo? = null,
    val pcName: String? = null,
    val profileGroups: List<LinkProfileGroup> = emptyList(),
    val profile: String? = null,
    val profileChanging: Boolean = false,
    val phoneFrameGen: Boolean = false,
    val phoneFrameGenAvailability: PhoneFrameGenAvailability = PhoneFrameGenAvailability.UNBOUND,
    val localEnhancementsOff: Boolean = false,
) {
    fun option(location: ProcessingLocation): ProcessingOption = options.firstOrNull { it.location == location } ?: ProcessingOption(location)
}

/** What the current phone playback can give the PC. */
sealed interface SendableState {
    /** A web stream, a debrid link or a torrent the PC can open. */
    data object Sendable : SendableState
    data object LocalFile : SendableState
    data object NotSendable : SendableState
    data object NothingYet : SendableState
}

/** Everything [processingOptions] decides from. */
internal data class ProcessingFacts(
    val current: ProcessingLocation,
    val paired: Boolean,
    val reachable: Boolean,
    val external: Boolean,
    /** Only matters to leave the phone (PC <-> Mixto changes the session's profile, the source is the PC's already). */
    val sendable: SendableState,
    /** The phone can play the title itself (a source flow, or a title it can look up). */
    val hasOriginal: Boolean,
    val mpvAvailable: Boolean,
    val phoneFrameGen: PhoneFrameGenAvailability,
    /** Capabilities `client_fg` (null = not known yet: not a reason to block). */
    val clientFgAllowed: Boolean?,
    /** The capabilities are known and the "Mixto" profile is [mixedProfile] (null with known capabilities = none). */
    val capabilitiesKnown: Boolean,
    val mixedProfile: String?,
)

/** The three options of "Procesar en" with the reason each one cannot be chosen (the current one is always available). */
internal fun processingOptions(f: ProcessingFacts): List<ProcessingOption> = ProcessingLocation.entries.map { loc ->
    if (loc == f.current) return@map ProcessingOption(loc)
    val (block, detail) = blockOf(loc, f)
    ProcessingOption(loc, block, detail)
}

private fun blockOf(loc: ProcessingLocation, f: ProcessingFacts): Pair<ProcessingBlock?, String?> {
    if (f.external) return ProcessingBlock.EXTERNAL_PLAYER to null
    if (loc == ProcessingLocation.PHONE) return (if (f.hasOriginal) null else ProcessingBlock.NO_ORIGINAL_SOURCE) to null
    if (!f.paired) return ProcessingBlock.NOT_PAIRED to null
    if (!f.reachable) return ProcessingBlock.PC_OFFLINE to null
    if (f.current == ProcessingLocation.PHONE) {
        when (f.sendable) {
            SendableState.Sendable -> Unit
            SendableState.LocalFile -> return ProcessingBlock.LOCAL_FILE to null
            SendableState.NotSendable -> return ProcessingBlock.SOURCE_NOT_SENDABLE to null
            SendableState.NothingYet -> return ProcessingBlock.NO_SOURCE_YET to null
        }
    }
    if (loc == ProcessingLocation.MIXED) {
        if (!f.mpvAvailable) return ProcessingBlock.MPV_UNAVAILABLE to null
        if (!f.phoneFrameGen.eligible) return ProcessingBlock.PHONE_FG_UNAVAILABLE to f.phoneFrameGen.reason
        if (f.clientFgAllowed == false) return ProcessingBlock.PC_SUPPRESSES_PHONE_FG to null
        if (f.capabilitiesKnown && f.mixedProfile == null) return ProcessingBlock.PC_NO_UPSCALE_PROFILE to null
    }
    return null to null
}

/**
 * The engine a location plays on. MIXED needs libmpv (the phone's frame generation runs on it), through the loopback proxy.
 * PC stays on Media3 by default: the phone processes nothing there, so libmpv's shaders and frame generation add nothing, and
 * Media3 has the Link-specific handling QA'd in 0.3.x (RemoteLoadErrorPolicy for the PC's 503 / 404 / 410 answers, the decoder
 * rejection of a profile with a fallback profile, BUG-131 / BUG-132 / BUG-138). [pcOnMpv] switches PC to libmpv too
 * ([LINK_PC_ON_MPV]) once that path is QA'd. PHONE uses the engine local playback uses ([local]).
 */
internal fun engineFor(location: ProcessingLocation, local: PlayerEngine, mpvAvailable: Boolean, pcOnMpv: Boolean = LINK_PC_ON_MPV): PlayerEngine =
    when (location) {
        ProcessingLocation.PHONE -> local
        ProcessingLocation.MIXED -> if (mpvAvailable) PlayerEngine.MPV else PlayerEngine.MEDIA3
        ProcessingLocation.PC -> if (pcOnMpv && mpvAvailable && local == PlayerEngine.MPV) PlayerEngine.MPV else PlayerEngine.MEDIA3
    }

/** See [engineFor]: false = strict "PC" plays on Media3 (mTLS client), only "Mixto" on libmpv (loopback proxy). */
internal const val LINK_PC_ON_MPV = false

/**
 * Where a new player starts: a PC hand-off / "Enviar a dispositivo" plays from the PC; else the location the previous episode of
 * this title used in this run ([carried]); else Settings › Enlace › "Procesar por defecto en" when a PC is paired; else the phone.
 */
internal fun startLocation(remoteSession: Boolean, carried: ProcessingLocation?, default: ProcessingLocation?, paired: Boolean): ProcessingLocation = when {
    remoteSession -> ProcessingLocation.PC
    carried != null -> carried
    paired && default != null -> default
    else -> ProcessingLocation.PHONE
}

/**
 * The location chosen in the player carries over to the next / previous episode opened from it (a new player), for this run
 * of the app only: the saved default stays what Settings says.
 */
internal object ProcessingCarry {
    @Volatile private var carried: Pair<String, ProcessingLocation>? = null

    fun put(titleKey: String, location: ProcessingLocation) {
        carried = titleKey to location
    }

    /** The location carried for [titleKey] (once). */
    fun take(titleKey: String): ProcessingLocation? {
        val c = carried ?: return null
        carried = null
        return c.second.takeIf { c.first == titleKey }
    }
}
