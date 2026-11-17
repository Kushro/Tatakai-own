// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityRequest
import app.tatakai.next.player.ShaderFamily
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (worker A): chain building per preset / tier / resolution and every guard of the planner.
class QualityPlannerTest {
    /** 720p source on a 2800x1752 tablet: upscaled ~2.4x. */
    private val tablet720 = QualitySignals(videoWidth = 1280, videoHeight = 720, viewportWidth = 2800, viewportHeight = 1752)
    private fun req(preset: String, tier: QualityTier = QualityTier.BALANCED, deband: Boolean = true) =
        QualityRequest(preset = preset, tier = tier, deband = DebandConfig(enabled = deband))

    private fun names(p: QualityPlan) = p.shaders.map { it.asset }

    @Test
    fun anime4kATiersUseTheirNetworks() {
        val fast = QualityPlanner.plan(req("anime4k-a", QualityTier.FAST), tablet720)
        assertEquals(
            listOf("Anime4K_Clamp_Highlights.glsl", "Anime4K_Restore_CNN_S.glsl", "Anime4K_Upscale_CNN_x2_S.glsl", "Anime4K_AutoDownscalePre_x2.glsl", "Anime4K_Upscale_CNN_x2_S.glsl"),
            names(fast),
        )
        val balanced = QualityPlanner.plan(req("anime4k-a"), tablet720)
        // The 0.3.x chain of the A preset (desktop "Fast" M/S).
        assertEquals(
            listOf("Anime4K_Clamp_Highlights.glsl", "Anime4K_Restore_CNN_M.glsl", "Anime4K_Upscale_CNN_x2_M.glsl", "Anime4K_AutoDownscalePre_x2.glsl", "Anime4K_Upscale_CNN_x2_S.glsl"),
            names(balanced),
        )
        val hq = QualityPlanner.plan(req("anime4k-a", QualityTier.HQ), tablet720)
        assertEquals("Anime4K_Restore_CNN_L.glsl", names(hq)[1])
        assertEquals("Anime4K_Upscale_CNN_x2_M.glsl", names(hq).last())
        assertEquals(QualityLevel("anime4k-a", QualityTier.HQ), hq.level)
        assertEquals(ShaderFamily.ANIME4K, hq.family)
        assertTrue(hq.reasons.isEmpty())
        assertTrue(hq.deband)
    }

    @Test
    fun everyDesktopPresetHasAChainAtEveryTier() {
        for (id in QualityPresets.ALL - setOf(QualityPresets.OFF, QualityPresets.ARTCNN)) {
            for (t in QualityTier.entries) assertTrue(ShaderCatalog.chain(id, t).isNotEmpty(), "$id $t")
        }
        assertEquals(listOf("FSRCNNX_x2_8-0-4-1_LineArt.glsl", "KrigBilateral.glsl", "SSimSuperRes.glsl", "SSimDownscaler.glsl"), ShaderCatalog.chain("fidelity", QualityTier.HQ))
        assertEquals(listOf("SSimSuperRes.glsl"), ShaderCatalog.chain("fidelity", QualityTier.FAST))
        val ca = ShaderCatalog.chain("anime4k-ca", QualityTier.BALANCED)
        assertEquals(listOf("Anime4K_Clamp_Highlights.glsl", "Anime4K_Upscale_Denoise_CNN_x2_M.glsl", "Anime4K_AutoDownscalePre_x2.glsl", "Anime4K_Restore_CNN_S.glsl", "Anime4K_Upscale_CNN_x2_S.glsl"), ca)
    }

    @Test
    fun untieredPresetsIgnoreTheTier() {
        val fsr = QualityPlanner.plan(req("fsr", QualityTier.HQ), tablet720)
        assertEquals(QualityLevel("fsr", null), fsr.level)
        assertEquals(listOf("FSR.glsl"), names(fsr))
    }

    @Test
    fun artCnnIsNeverRunWithThisRenderer() {
        val p = QualityPlanner.plan(req("artcnn"), tablet720)
        assertTrue(p.level.off)
        assertTrue(p.shaders.isEmpty())
        assertEquals(listOf(QualityReason.RENDERER_UNSUPPORTED), p.reasons)
    }

    @Test
    fun unknownSizeWaitsForMetadata() {
        val p = QualityPlanner.plan(req("anime4k-aa"), QualitySignals())
        assertTrue(p.level.off)
        assertEquals(listOf(QualityReason.METADATA_PENDING), p.reasons)
        assertTrue(p.deband, "deband does not need the size")
    }

    @Test
    fun resolutionGuards() {
        val uhd = QualityPlanner.plan(req("anime4k-a"), tablet720.copy(videoWidth = 3840, videoHeight = 2160))
        assertTrue(uhd.level.off)
        assertFalse(uhd.deband)
        assertEquals(listOf(QualityReason.RES_4K), uhd.reasons)
        val eightK = QualityPlanner.plan(req("fidelity"), tablet720.copy(videoWidth = 7680, videoHeight = 4320))
        assertEquals(listOf(QualityReason.RES_8K), eightK.reasons)
        assertTrue(eightK.shaders.isEmpty())
        // Portrait 1080x1920 classifies as FHD, not 4K.
        assertEquals(ResolutionTier.FHD, ResolutionTier.fromDimensions(1080, 1920))
    }

    @Test
    fun viewportGuardDropsUpscalersOnly() {
        // 1080p on a 1080p-tall phone in landscape: scale 1.0, nothing to upscale.
        val phone = QualitySignals(videoWidth = 1920, videoHeight = 1080, viewportWidth = 2400, viewportHeight = 1080)
        val a = QualityPlanner.plan(req("anime4k-a"), phone)
        assertTrue(a.level.off)
        assertEquals(listOf(QualityReason.VIEWPORT), a.reasons)
        val fidelity = QualityPlanner.plan(req("fidelity", QualityTier.HQ), phone)
        assertEquals(listOf("KrigBilateral.glsl", "SSimDownscaler.glsl"), names(fidelity))
        assertEquals(QualityLevel("fidelity", QualityTier.HQ), fidelity.level)
        val cas = QualityPlanner.plan(req("cas"), phone)
        assertEquals(listOf("CAS.glsl"), names(cas))
        assertEquals(ShaderTransform.CAS, cas.shaders.single().transform)
        assertEquals(SharpenLevel.MEDIUM.casSharpening, cas.shaders.single().casSharpening)
        // Unknown view: no guard.
        assertTrue(QualityPlanner.upscalesInView(QualitySignals(videoWidth = 1920, videoHeight = 1080)))
    }

    @Test
    fun batteryEcoTurnsEverythingOffOnlyWithAutoAdjust() {
        val r = req("anime4k-a").copy(smoothing = true, sharpen = SharpenLevel.LOW)
        val low = QualityPlanner.plan(r, tablet720.copy(batteryLow = true))
        assertTrue(low.level.off)
        assertFalse(low.deband)
        assertFalse(low.interpolation)
        assertFalse(low.sharpen)
        assertEquals(listOf(QualityReason.BATTERY_LOW), low.reasons)
        val manual = QualityPlanner.plan(r.copy(autoAdjust = false), tablet720.copy(batteryLow = true))
        assertFalse(manual.level.off)
        assertTrue(manual.interpolation)
    }

    @Test
    fun thermalLowersOneTierButNeverTurnsOff() {
        val hq = QualityPlanner.plan(req("anime4k-a", QualityTier.HQ), tablet720.copy(thermal = true))
        assertEquals(QualityTier.BALANCED, hq.level.tier)
        assertEquals(listOf(QualityReason.THERMAL), hq.reasons)
        val fast = QualityPlanner.plan(req("anime4k-a", QualityTier.FAST), tablet720.copy(thermal = true))
        assertEquals(QualityTier.FAST, fast.level.tier)
        assertTrue(fast.reasons.isEmpty())
    }

    @Test
    fun pipCapsAtFast() {
        val p = QualityPlanner.plan(req("anime4k-b", QualityTier.HQ), tablet720.copy(pip = true))
        assertEquals(QualityTier.FAST, p.level.tier)
        assertEquals(listOf(QualityReason.PIP), p.reasons)
    }

    @Test
    fun remoteProcessingAndFrameGen() {
        val r = req("anime4k-a").copy(smoothing = true)
        val remote = QualityPlanner.plan(r, tablet720.copy(remoteProcessing = true))
        assertTrue(remote.level.off)
        assertFalse(remote.deband)
        assertFalse(remote.interpolation)
        assertEquals(listOf(QualityReason.REMOTE_PROCESSING), remote.reasons)
        val fg = QualityPlanner.plan(r, tablet720.copy(frameGenActive = true))
        assertFalse(fg.level.off, "frame generation keeps the upscaler")
        assertFalse(fg.interpolation)
        assertEquals(listOf(QualityReason.FRAME_GEN), fg.reasons)
    }

    @Test
    fun shaderErrorTurnsTheChainOff() {
        val p = QualityPlanner.plan(req("anime4k-a").copy(sharpen = SharpenLevel.HIGH), tablet720.copy(shaderError = true))
        assertTrue(p.shaders.isEmpty())
        assertEquals(listOf(QualityReason.SHADER_ERROR), p.reasons)
    }

    @Test
    fun sharpenIsAppendedAfterTheChain() {
        val p = QualityPlanner.plan(req("anime4k-c").copy(sharpen = SharpenLevel.HIGH), tablet720)
        assertEquals(ShaderSpec("CAS.glsl", ShaderTransform.CAS, 1.0), p.shaders.last())
        assertTrue(p.sharpen)
        val only = QualityPlanner.plan(req("off").copy(sharpen = SharpenLevel.LOW), QualitySignals())
        assertEquals(listOf("CAS.glsl"), names(only))
        assertTrue(only.level.off)
    }

    @Test
    fun dropLadderStepsDownThenOffThenExtras() {
        val r = req("anime4k-a", QualityTier.HQ).copy(smoothing = true)
        var ceiling: DropCeiling? = null
        val seen = mutableListOf<QualityLevel>()
        repeat(4) {
            val plan = QualityPlanner.plan(r, tablet720.copy(dropCeiling = ceiling))
            seen += plan.level
            ceiling = QualityPlanner.nextDropCeiling(ceiling, plan) ?: return@repeat
        }
        assertEquals(
            listOf(
                QualityLevel("anime4k-a", QualityTier.HQ),
                QualityLevel("anime4k-a", QualityTier.BALANCED),
                QualityLevel("anime4k-a", QualityTier.FAST),
                QualityLevel.OFF,
            ),
            seen,
        )
        val last = QualityPlanner.plan(r, tablet720.copy(dropCeiling = ceiling))
        assertFalse(last.interpolation)
        assertFalse(last.deband)
        assertEquals(listOf(QualityReason.FRAME_DROPS), last.reasons)
        assertNull(QualityPlanner.nextDropCeiling(ceiling, last))
        // autoAdjust off ignores the ceiling.
        assertEquals(QualityTier.HQ, QualityPlanner.plan(r.copy(autoAdjust = false), tablet720.copy(dropCeiling = ceiling)).level.tier)
    }

    @Test
    fun reasonsAreOrderedByImportance() {
        val p = QualityPlanner.plan(req("anime4k-a", QualityTier.HQ), tablet720.copy(thermal = true, pip = true))
        assertEquals(listOf(QualityReason.THERMAL, QualityReason.PIP), p.reasons)
    }
}
