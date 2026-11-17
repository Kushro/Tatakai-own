// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkFrameGen
import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.model.LinkUpscale
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.player.QualityEvent
import app.tatakai.next.player.QualityEventKind
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityUiState

// 0.4.0 (worker D, artifacts/player-quality-0.4.0-specs/d-player-ui.md): the status chip of the player's top bar says, at a glance,
// WHAT enhancement runs and WHERE (phone / PC / both). Modelled on Enigmata's StatusChips.kt + TemplePill.kt (AGPL-3.0-or-later, same
// author). Pure: state -> [EnhancementChip] -> text through [ChipWords] (resources in the app, plain English in the unit tests).

/** What the chip shows. Built by [enhancementChip]; [chipLabel] turns it into text. */
sealed interface EnhancementChip {
    /** libmpv on the phone: [level] is what runs (or what was asked, while [PhoneStatus.PAUSED]); [phoneFg] = live FG multiplier. */
    data class Phone(
        val level: QualityLevel,
        val status: PhoneStatus,
        val phoneFg: Int? = null,
        /** With the preset off: the cheap extras still running (deband, sharpen, smoothing), so "Sin mejoras" is never a lie. */
        val extras: List<PhoneExtra> = emptyList(),
    ) : EnhancementChip

    /** Media3 plays on the phone: no enhancements (the panel offers "Cambiar a MPV"). */
    data object Media3 : EnhancementChip

    /** Strict PC: what the PC does ([info] null = not asked yet). */
    data class Pc(val info: LinkProcessingInfo?) : EnhancementChip

    /** Mixto: the PC upscales, the phone generates frames ([phoneFg] = live multiplier, null while it is not presenting). */
    data class Mixed(val phoneFg: Int?, val phoneFgProblem: Boolean) : EnhancementChip

    /** A "Procesar en" switch runs. */
    data class Switching(val to: ProcessingLocation) : EnhancementChip
}

enum class PhoneStatus {
    /** What the user asked for runs. */
    RUNNING,

    /** The preset is "off" (by choice). */
    OFF,

    /** Off for a harmless reason (the video already fills the view, a 4K source, its size is not known yet): nothing is wrong. */
    PAUSED,

    /** The engine lowered or suspended it (heat, battery, frame drops, a shader error, 8K, PiP): warning icon, the panel says why. */
    RESTRICTED,
}

enum class PhoneExtra { DEBAND, SHARPEN, SMOOTHING }

/** Visual weight of the chip. */
enum class ChipTone { NEUTRAL, ACTIVE, WARNING }

/** Reasons that make the chip say "Restringido" (the others are expected and explained in the panel only). */
internal val RESTRICTING_REASONS: Set<QualityReason> = setOf(
    QualityReason.SHADER_ERROR,
    QualityReason.RES_8K,
    QualityReason.BATTERY_LOW,
    QualityReason.THERMAL,
    QualityReason.FRAME_DROPS,
    QualityReason.PIP,
    QualityReason.RENDERER_UNSUPPORTED,
)

/**
 * The chip for the current playback. [location] / [switchingTo] / [pc] come from `PlayerViewModel.processing`, [quality] from the
 * session, [phoneFgMultiplier] is the phone FG's live N (only while it presents), [phoneFgProblem] = FG is on but cannot run
 * (ineligible or failed, not a temporary pause).
 */
fun enhancementChip(
    location: ProcessingLocation,
    switchingTo: ProcessingLocation?,
    pc: LinkProcessingInfo?,
    quality: QualityUiState,
    phoneFgMultiplier: Int?,
    phoneFgProblem: Boolean = false,
): EnhancementChip {
    if (switchingTo != null) return EnhancementChip.Switching(switchingTo)
    return when (location) {
        ProcessingLocation.PC -> EnhancementChip.Pc(pc)
        ProcessingLocation.MIXED -> EnhancementChip.Mixed(phoneFgMultiplier, phoneFgProblem)
        ProcessingLocation.PHONE -> if (!quality.available) EnhancementChip.Media3 else phoneChip(quality, phoneFgMultiplier)
    }
}

private fun phoneChip(q: QualityUiState, fg: Int?): EnhancementChip.Phone {
    val requested = QualityLevel.of(q.requested.preset, q.requested.tier)
    val effective = q.effective
    val restricted = q.reasons.any { it in RESTRICTING_REASONS }
    return when {
        requested.off -> EnhancementChip.Phone(
            level = QualityLevel.OFF,
            status = PhoneStatus.OFF,
            phoneFg = fg,
            extras = buildList {
                if (q.debandActive) add(PhoneExtra.DEBAND)
                if (q.sharpenActive) add(PhoneExtra.SHARPEN)
                if (q.smoothingActive) add(PhoneExtra.SMOOTHING)
            },
        )
        effective == requested -> EnhancementChip.Phone(effective, PhoneStatus.RUNNING, fg)
        restricted -> EnhancementChip.Phone(effective, PhoneStatus.RESTRICTED, fg)
        // Viewport / 4K / size pending: say what was asked, "en pausa" (no warning: portrait often needs no upscaling).
        effective.off -> EnhancementChip.Phone(requested, PhoneStatus.PAUSED, fg)
        else -> EnhancementChip.Phone(effective, PhoneStatus.RUNNING, fg)
    }
}

fun chipTone(chip: EnhancementChip): ChipTone = when (chip) {
    is EnhancementChip.Phone -> when (chip.status) {
        PhoneStatus.RESTRICTED -> ChipTone.WARNING
        PhoneStatus.RUNNING -> ChipTone.ACTIVE
        PhoneStatus.OFF, PhoneStatus.PAUSED -> if (chip.phoneFg != null) ChipTone.ACTIVE else ChipTone.NEUTRAL
    }
    EnhancementChip.Media3 -> ChipTone.NEUTRAL
    is EnhancementChip.Pc -> if (chip.info?.fallingBehind == true || chip.info?.degradedFrom != null) ChipTone.WARNING else ChipTone.ACTIVE
    is EnhancementChip.Mixed -> if (chip.phoneFgProblem) ChipTone.WARNING else ChipTone.ACTIVE
    is EnhancementChip.Switching -> ChipTone.NEUTRAL
}

/** The words of the chip (es / en resources in the app; the unit tests pass plain English). */
interface ChipWords {
    /** "Anime4K A+A", "FSR", "Fidelity"... */
    fun preset(id: String): String

    /** "Rápido", "Equilibrado", "HQ". */
    fun tier(tier: QualityTier): String
    val noEnhancements: String
    val restricted: String
    val paused: String

    /** "FG ×2". */
    fun frameGen(multiplier: Int): String
    val deband: String
    val sharpen: String
    val smoothing: String
    val media3: String

    /** "PC". */
    val pc: String

    /** "sin mejoras" after "PC · ". */
    val pcNothing: String

    /** "Anime4K 1080p" ([height] null = unknown output). */
    fun pcAnime4k(height: Int?): String

    /** "LSFG ×2" ([multiplier] null = the PC did not say), "LSFG auto". */
    fun pcLsfg(multiplier: Int?): String
    val pcLsfgAuto: String

    /** AMD FRC (always ×2). */
    val pcFrc: String

    /** "PC escala". */
    val mixedPcScales: String

    /** "Teléfono FG ×2" ([multiplier] null = not presenting yet). */
    fun mixedPhoneFg(multiplier: Int?): String

    /** "Cambiando a la PC…". */
    fun switching(to: ProcessingLocation): String
}

const val CHIP_SEPARATOR = " · "

fun chipLabel(chip: EnhancementChip, w: ChipWords): String = when (chip) {
    is EnhancementChip.Switching -> w.switching(chip.to)
    EnhancementChip.Media3 -> w.media3
    is EnhancementChip.Pc -> w.pc + CHIP_SEPARATOR + pcWhat(chip.info, w)
    is EnhancementChip.Mixed -> w.mixedPcScales + CHIP_SEPARATOR + w.mixedPhoneFg(chip.phoneFg)
    is EnhancementChip.Phone -> phoneLabel(chip, w)
}

private fun phoneLabel(chip: EnhancementChip.Phone, w: ChipWords): String {
    val fg = chip.phoneFg?.let { w.frameGen(it) }
    val parts = when (chip.status) {
        PhoneStatus.OFF -> chip.extras.map {
            when (it) {
                PhoneExtra.DEBAND -> w.deband
                PhoneExtra.SHARPEN -> w.sharpen
                PhoneExtra.SMOOTHING -> w.smoothing
            }
        } + listOfNotNull(fg)
        PhoneStatus.RESTRICTED -> if (chip.level.off) listOfNotNull(w.restricted, fg) else levelParts(chip.level, w) + listOfNotNull(fg, w.restricted)
        PhoneStatus.PAUSED -> levelParts(chip.level, w) + w.paused + listOfNotNull(fg)
        PhoneStatus.RUNNING -> levelParts(chip.level, w) + listOfNotNull(fg)
    }
    return parts.ifEmpty { listOf(w.noEnhancements) }.joinToString(CHIP_SEPARATOR)
}

private fun levelParts(level: QualityLevel, w: ChipWords): List<String> =
    if (level.off) emptyList() else listOfNotNull(w.preset(level.preset), level.tier?.let { w.tier(it) })

/** "Anime4K 1080p + LSFG ×2", "LSFG ×3", "sin mejoras"; "…" before the PC was asked. */
private fun pcWhat(info: LinkProcessingInfo?, w: ChipWords): String {
    if (info == null) return PC_PENDING
    val upscale = when (info.upscale) {
        is LinkUpscale.Anime4k -> w.pcAnime4k(info.outputHeight)
        LinkUpscale.None -> null
    }
    val fg = when (val f = info.frameGen) {
        LinkFrameGen.None -> null
        LinkFrameGen.FrcAmf -> w.pcFrc
        is LinkFrameGen.Lsfg -> w.pcLsfg(f.multiplier)
        // the effective multiplier the PC chose for this video; "auto" until it says (older PC, or the first poll)
        is LinkFrameGen.LsfgAuto -> f.multiplier?.let { w.pcLsfg(it) } ?: w.pcLsfgAuto
    }
    return listOfNotNull(upscale, fg).joinToString(" + ").ifEmpty { w.pcNothing }
}

/** Before the first status poll the chip only says "PC · …". */
internal const val PC_PENDING = "…"

/** The preset ids the in-player grid offers, in the desktop order (ArtCNN last, shown disabled with its reason on this build). */
val PANEL_PRESETS: List<String> = QualityPresets.ALL

/**
 * Whether an automatic quality change is worth a notice over the video ([QualityUiState.lastEvent]). Silent: the view guard
 * (rotation: portrait often needs no upscaling, the panel explains it), a 4K source (same: nothing to upscale), the video size arriving, PiP (nobody sees a notice there),
 * Link (the "Procesar en" notice already says it), frame generation turning smoothing off (the user just turned FG on) and the FP16
 * precision fallback (nothing visible changes). Restores are announced only for the reasons that announced the loss.
 */
fun announceQualityEvent(e: QualityEvent): Boolean {
    if (e.kind == QualityEventKind.PRECISION_FALLBACK) return false
    return when (e.reason) {
        QualityReason.VIEWPORT, QualityReason.METADATA_PENDING, QualityReason.PIP, QualityReason.REMOTE_PROCESSING,
        QualityReason.FRAME_GEN, QualityReason.RENDERER_UNSUPPORTED, QualityReason.RES_4K -> false
        QualityReason.BATTERY_LOW, QualityReason.THERMAL -> true
        QualityReason.FRAME_DROPS, QualityReason.SHADER_ERROR, QualityReason.RES_8K ->
            e.kind != QualityEventKind.RESTORED
    }
}
