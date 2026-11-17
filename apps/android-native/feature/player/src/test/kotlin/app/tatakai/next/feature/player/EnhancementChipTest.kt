// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkFrameGen
import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.model.LinkUpscale
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.ProcessingLocation.MIXED
import app.tatakai.next.model.ProcessingLocation.PC
import app.tatakai.next.model.ProcessingLocation.PHONE
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.player.QualityEvent
import app.tatakai.next.player.QualityEventKind
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityRequest
import app.tatakai.next.player.QualityUiState
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** 0.4.0 (worker D): the player's status chip, state -> text, and which automatic quality changes get a notice. */
class EnhancementChipTest {
    /** Plain English words (the app uses the es / en resources with the same shape). */
    private val words = object : ChipWords {
        override fun preset(id: String) = when (id) {
            QualityPresets.ANIME4K_AA -> "Anime4K A+A"
            QualityPresets.ANIME4K_A -> "Anime4K A"
            QualityPresets.FSR -> "FSR"
            else -> id
        }
        override fun tier(tier: QualityTier) = when (tier) {
            QualityTier.FAST -> "Fast"
            QualityTier.BALANCED -> "Balanced"
            QualityTier.HQ -> "HQ"
        }
        override val noEnhancements = "No enhancements"
        override val restricted = "Restricted"
        override val paused = "paused"
        override fun frameGen(multiplier: Int) = "FG ×$multiplier"
        override val deband = "Deband"
        override val sharpen = "Sharpening"
        override val smoothing = "Smoothing"
        override val media3 = "Media3 · no enhancements"
        override val pc = "PC"
        override val pcNothing = "no enhancements"
        override fun pcAnime4k(height: Int?) = if (height != null) "Anime4K ${height}p" else "Anime4K"
        override fun pcLsfg(multiplier: Int?) = if (multiplier != null) "LSFG ×$multiplier" else "LSFG"
        override val pcLsfgAuto = "LSFG auto"
        override val pcFrc = "AMD FRC ×2"
        override val mixedPcScales = "PC upscales"
        override fun mixedPhoneFg(multiplier: Int?) = if (multiplier != null) "Phone FG ×$multiplier" else "Phone FG"
        override fun switching(to: ProcessingLocation) = "Switching to $to…"
    }

    private fun mpv(
        preset: String = QualityPresets.ANIME4K_AA,
        tier: QualityTier = QualityTier.HQ,
        effective: QualityLevel = QualityLevel.of(preset, tier),
        reasons: List<QualityReason> = emptyList(),
        deband: Boolean = false,
    ) = QualityUiState(
        available = true,
        playerEngine = PlayerEngine.MPV,
        requested = QualityRequest(preset = preset, tier = tier),
        effective = effective,
        reasons = reasons,
        debandActive = deband,
    )

    private fun label(
        quality: QualityUiState = mpv(),
        location: ProcessingLocation = PHONE,
        fg: Int? = null,
        pc: LinkProcessingInfo? = null,
        switching: ProcessingLocation? = null,
        fgProblem: Boolean = false,
    ) = chipLabel(enhancementChip(location, switching, pc, quality, fg, fgProblem), words)

    private fun pcInfo(upscale: LinkUpscale = LinkUpscale.Anime4k("a", "m"), height: Int? = 1080, fg: LinkFrameGen = LinkFrameGen.None) =
        LinkProcessingInfo(
            pcName = "Desk",
            profileId = "p",
            profileLabel = null,
            upscale = upscale,
            outputWidth = height?.let { it * 16 / 9 },
            outputHeight = height,
            frameGen = fg,
            encoder = null,
            videoKbps = null,
        )

    @Test
    fun `phone runs the chosen preset and tier`() {
        assertEquals("Anime4K A+A · HQ", label())
        assertEquals(ChipTone.ACTIVE, chipTone(enhancementChip(PHONE, null, null, mpv(), null)))
    }

    @Test
    fun `phone frame generation is appended`() {
        assertEquals("Anime4K A+A · HQ · FG ×2", label(fg = 2))
    }

    @Test
    fun `untiered presets have no tier`() {
        assertEquals("FSR", label(mpv(preset = QualityPresets.FSR, effective = QualityLevel.of(QualityPresets.FSR, QualityTier.HQ))))
    }

    @Test
    fun `preset off says no enhancements, or the extras that still run`() {
        assertEquals("No enhancements", label(mpv(preset = QualityPresets.OFF)))
        assertEquals("Deband", label(mpv(preset = QualityPresets.OFF, deband = true)))
        assertEquals("FG ×3", label(mpv(preset = QualityPresets.OFF), fg = 3))
    }

    @Test
    fun `suspended by the engine is restricted with a warning`() {
        val q = mpv(effective = QualityLevel.OFF, reasons = listOf(QualityReason.BATTERY_LOW))
        val chip = enhancementChip(PHONE, null, null, q, null)
        assertEquals("Restricted", chipLabel(chip, words))
        assertEquals(ChipTone.WARNING, chipTone(chip))
    }

    @Test
    fun `stepped down says what runs and that it is restricted`() {
        val q = mpv(effective = QualityLevel.of(QualityPresets.ANIME4K_AA, QualityTier.FAST), reasons = listOf(QualityReason.FRAME_DROPS))
        assertEquals("Anime4K A+A · Fast · Restricted", label(q))
    }

    @Test
    fun `the view guard only pauses, without a warning`() {
        val q = mpv(effective = QualityLevel.OFF, reasons = listOf(QualityReason.VIEWPORT))
        val chip = enhancementChip(PHONE, null, null, q, null)
        assertEquals("Anime4K A+A · HQ · paused", chipLabel(chip, words))
        assertEquals(ChipTone.NEUTRAL, chipTone(chip))
    }

    @Test
    fun `media3 has no enhancements`() {
        assertEquals("Media3 · no enhancements", label(QualityUiState.MEDIA3))
    }

    @Test
    fun `pc says what the pc does`() {
        assertEquals("PC · Anime4K 1080p", label(location = PC, pc = pcInfo()))
        assertEquals("PC · Anime4K 1080p + LSFG ×2", label(location = PC, pc = pcInfo(fg = LinkFrameGen.Lsfg(2))))
        assertEquals("PC · LSFG ×3", label(location = PC, pc = pcInfo(upscale = LinkUpscale.None, fg = LinkFrameGen.Lsfg(3))))
        assertEquals("PC · no enhancements", label(location = PC, pc = pcInfo(upscale = LinkUpscale.None)))
        assertEquals("PC · AMD FRC ×2", label(location = PC, pc = pcInfo(upscale = LinkUpscale.None, fg = LinkFrameGen.FrcAmf)))
        assertEquals("PC · …", label(location = PC))
    }

    @Test
    fun `pc auto lsfg shows the effective multiplier once known`() {
        assertEquals("PC · Anime4K 1080p + LSFG ×5", label(location = PC, pc = pcInfo(fg = LinkFrameGen.LsfgAuto(120, multiplier = 5, outFps = 119.88))))
        assertEquals("PC · Anime4K 1080p + LSFG auto", label(location = PC, pc = pcInfo(fg = LinkFrameGen.LsfgAuto(120))))
    }

    @Test
    fun `pc wins over the media3 engine it plays on`() {
        assertEquals("PC · Anime4K 1080p", label(QualityUiState.MEDIA3, location = PC, pc = pcInfo()))
    }

    @Test
    fun `mixed says who does what`() {
        assertEquals("PC upscales · Phone FG ×2", label(location = MIXED, fg = 2))
        assertEquals("PC upscales · Phone FG", label(location = MIXED))
        assertEquals(ChipTone.WARNING, chipTone(enhancementChip(MIXED, null, null, mpv(), null, phoneFgProblem = true)))
    }

    @Test
    fun `a switch in progress wins`() {
        assertEquals("Switching to PC…", label(location = PHONE, switching = PC))
    }

    private fun event(reason: QualityReason, kind: QualityEventKind) =
        QualityEvent(1, reason, kind, QualityLevel.of(QualityPresets.ANIME4K_AA, QualityTier.HQ), QualityLevel.OFF)

    @Test
    fun `quality notices skip what is expected and announce what the user should know`() {
        assertFalse(announceQualityEvent(event(QualityReason.VIEWPORT, QualityEventKind.SUSPENDED)))
        assertFalse(announceQualityEvent(event(QualityReason.VIEWPORT, QualityEventKind.RESTORED)))
        assertFalse(announceQualityEvent(event(QualityReason.RES_4K, QualityEventKind.SUSPENDED)))
        assertFalse(announceQualityEvent(event(QualityReason.PIP, QualityEventKind.DOWNGRADED)))
        assertFalse(announceQualityEvent(event(QualityReason.REMOTE_PROCESSING, QualityEventKind.SUSPENDED)))
        assertFalse(announceQualityEvent(event(QualityReason.SHADER_ERROR, QualityEventKind.PRECISION_FALLBACK)))
        assertTrue(announceQualityEvent(event(QualityReason.FRAME_DROPS, QualityEventKind.DOWNGRADED)))
        assertTrue(announceQualityEvent(event(QualityReason.BATTERY_LOW, QualityEventKind.SUSPENDED)))
        assertTrue(announceQualityEvent(event(QualityReason.BATTERY_LOW, QualityEventKind.RESTORED)))
        assertTrue(announceQualityEvent(event(QualityReason.THERMAL, QualityEventKind.DOWNGRADED)))
        assertTrue(announceQualityEvent(event(QualityReason.SHADER_ERROR, QualityEventKind.SUSPENDED)))
        assertTrue(announceQualityEvent(event(QualityReason.RES_8K, QualityEventKind.SUSPENDED)))
    }
}
