// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.QualityTier
import app.tatakai.next.player.QualityEventKind
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityRequest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (worker A): the controller against a fake mpv: live writes (only what changed), events, undo, FP16 fallback, telemetry.
class MpvQualityControllerTest {
    private class FakePort : QualityMpvPort {
        val props = HashMap<String, String>()
        val writes = mutableListOf<Pair<String, String>>()
        override fun setProperty(name: String, value: String) {
            props[name] = value
            writes += name to value
        }

        override fun getProperty(name: String): String? = props[name]
    }

    /** Resolves to `/s/<relative path>` without touching the disk. */
    private val resolver = ShaderResolver { specs, fp16 -> ShaderNames.relativePaths(specs, fp16).map { "/s/$it" } }
    private var clock = 0L

    private fun controller(port: FakePort, r: QualityRequest = QualityRequest(preset = "anime4k-a"), fp16: Boolean = false) =
        MpvQualityController(port, resolver, r, fp16 = fp16, now = { clock })

    private fun MpvQualityController.tablet720() {
        onVideoSize(1280, 720)
        onViewport(2800, 1752)
    }

    @Test
    fun appliesTheChainOnceTheSizeIsKnown() {
        val port = FakePort()
        val c = controller(port)
        assertEquals("", port.props["glsl-shaders"])
        assertEquals(listOf(QualityReason.METADATA_PENDING), c.state.value.reasons)
        assertEquals("yes", port.props["deband"])
        assertEquals(DecoderMode.AUTO.hwdec, port.props["hwdec"])
        c.tablet720()
        val shaders = port.props["glsl-shaders"]!!.split(":")
        assertEquals(5, shaders.size)
        assertTrue(shaders.first().endsWith("Anime4K_Clamp_Highlights.glsl"))
        val s = c.state.value
        assertTrue(s.available)
        assertEquals(QualityLevel("anime4k-a", QualityTier.BALANCED), s.effective)
        assertEquals(5, s.activeShaders.size)
        assertNull(s.lastEvent, "waiting for the size is never announced")
    }

    @Test
    fun liveChangesWriteOnlyWhatChanged() {
        val port = FakePort()
        val c = controller(port)
        c.tablet720()
        port.writes.clear()
        c.setImage(ImageAdjust(brightness = 10))
        assertEquals(listOf("brightness" to "10"), port.writes)
        port.writes.clear()
        c.setAspect(AspectMode.FILL, 100)
        assertEquals(listOf("panscan" to "1.0"), port.writes)
        port.writes.clear()
        c.setDecoder(DecoderMode.SW)
        assertEquals(listOf("hwdec" to "no"), port.writes)
        port.writes.clear()
        c.setQuality("anime4k-c", QualityTier.HQ)
        assertEquals(listOf("glsl-shaders"), port.writes.map { it.first })
        assertTrue(port.props["glsl-shaders"]!!.contains("Anime4K_Upscale_Denoise_CNN_x2_L.glsl"))
        port.writes.clear()
        c.setSmoothing(true)
        assertEquals("yes", port.props["interpolation"])
        assertEquals("display-resample", port.props["video-sync"])
        assertNull(c.state.value.lastEvent, "user changes are not announced")
    }

    @Test
    fun frameDropsStepDownWithAnUndoableEvent() {
        val port = FakePort()
        val c = controller(port, QualityRequest(preset = "anime4k-a", tier = QualityTier.HQ))
        c.tablet720()
        c.onDropSample(0, 24.0, 0)
        c.onDropSample(120, 24.0, 8_000)
        val s = c.state.value
        assertEquals(QualityTier.BALANCED, s.effective.tier)
        val e = assertNotNull(s.lastEvent)
        assertEquals(QualityReason.FRAME_DROPS, e.reason)
        assertEquals(QualityEventKind.DOWNGRADED, e.kind)
        assertTrue(e.undoable)
        assertEquals(QualityLevel("anime4k-a", QualityTier.HQ), assertNotNull(s.autoDowngrade).from)
        c.undoAutoDowngrade()
        assertEquals(QualityTier.HQ, c.state.value.effective.tier)
        assertNull(c.state.value.autoDowngrade)
        // No further step-down for this file after an undo.
        c.onDropSample(120, 24.0, 10_000)
        c.onDropSample(400, 24.0, 18_000)
        assertEquals(QualityTier.HQ, c.state.value.effective.tier)
        // A new file may step down again.
        c.onFileStarted()
        c.tablet720()
        c.onDropSample(0, 24.0, 20_000)
        c.onDropSample(200, 24.0, 28_000)
        assertEquals(QualityTier.BALANCED, c.state.value.effective.tier)
    }

    @Test
    fun batteryAndThermalAreAnnounced() {
        val port = FakePort()
        val c = controller(port, QualityRequest(preset = "anime4k-a", tier = QualityTier.HQ))
        c.tablet720()
        c.onThermal(true)
        assertEquals(QualityReason.THERMAL, c.state.value.lastEvent?.reason)
        assertEquals(QualityEventKind.DOWNGRADED, c.state.value.lastEvent?.kind)
        c.onBattery(15, charging = false)
        assertEquals(QualityReason.BATTERY_LOW, c.state.value.lastEvent?.reason)
        assertEquals(QualityEventKind.SUSPENDED, c.state.value.lastEvent?.kind)
        assertEquals("", port.props["glsl-shaders"])
        assertEquals("no", port.props["deband"])
        c.onBattery(15, charging = true)
        assertEquals(QualityEventKind.RESTORED, c.state.value.lastEvent?.kind)
        assertEquals(QualityTier.BALANCED, c.state.value.effective.tier, "still hot")
        val seqs = c.state.value.lastEvent!!.seq
        c.onBattery(16, charging = true)
        assertEquals(seqs, c.state.value.lastEvent!!.seq, "no change, no event")
    }

    @Test
    fun remoteProcessingClearsTheChainAndComesBack() {
        val port = FakePort()
        val c = controller(port)
        c.tablet720()
        c.setRemoteProcessing(true)
        assertEquals("", port.props["glsl-shaders"])
        assertTrue(c.state.value.remoteProcessing)
        assertEquals(QualityReason.REMOTE_PROCESSING, c.state.value.reasons.single())
        c.setRemoteProcessing(false)
        assertEquals(5, port.props["glsl-shaders"]!!.split(":").size)
    }

    @Test
    fun fp16FallsBackToFullPrecisionThenTurnsOff() {
        val port = FakePort()
        val c = controller(port, fp16 = true)
        c.tablet720()
        assertTrue(c.state.value.fp16)
        assertTrue(port.props["glsl-shaders"]!!.contains(ShaderNames.FP16_DIR))
        clock = 10_000
        c.onShaderError()
        assertFalse(c.state.value.fp16)
        assertFalse(port.props["glsl-shaders"]!!.contains(ShaderNames.FP16_DIR))
        assertEquals(QualityEventKind.PRECISION_FALLBACK, c.state.value.lastEvent?.kind)
        // Errors of the replaced chain within the grace window are ignored.
        clock = 11_000
        c.onShaderError()
        assertFalse(c.state.value.effective.off)
        // A later error on the full-precision chain turns it off for this file.
        clock = 20_000
        c.onShaderError()
        assertTrue(c.state.value.effective.off)
        assertEquals(QualityReason.SHADER_ERROR, c.state.value.lastEvent?.reason)
        assertEquals("", port.props["glsl-shaders"])
    }

    @Test
    fun telemetryIsPublished() {
        val port = FakePort()
        val c = controller(port)
        port.props += mapOf(
            "hwdec-current" to "mediacodec", "frame-drop-count" to "3", "decoder-frame-drop-count" to "2",
            "container-fps" to "23.976", "current-vo" to "gpu", "gpu-api" to "auto",
        )
        c.refreshTelemetry()
        c.onDisplayRefreshRate(120f)
        val s = c.state.value
        assertEquals("mediacodec", s.decoderInUse)
        assertEquals(5, s.droppedFrames)
        assertEquals(23.976, s.videoFps)
        assertEquals("gpu / opengl", s.renderer)
        assertEquals(120f, s.displayHz)
        assertFalse(s.presets.single { it.id == "artcnn" }.available)
        assertEquals(QualityReason.RENDERER_UNSUPPORTED, s.presets.single { it.id == "artcnn" }.unavailableReason)
        assertTrue(s.presets.single { it.id == "fidelity" }.available)
    }

    @Test
    fun failedInstallLeavesTheChainEmpty() {
        val port = FakePort()
        val c = MpvQualityController(port, { _, _ -> null }, QualityRequest(preset = "fsr"), fp16 = false)
        c.tablet720()
        assertEquals("", port.props["glsl-shaders"])
        assertTrue(c.state.value.activeShaders.isEmpty())
    }
}
