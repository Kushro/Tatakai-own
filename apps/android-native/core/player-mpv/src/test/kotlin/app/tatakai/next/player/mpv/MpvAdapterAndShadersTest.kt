// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import androidx.media3.common.Player
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave A (15 §5 p (g)): MpvPlayerAdapter state derived from a fake MpvState, and the bundled shader chains.
class MpvAdapterAndShadersTest {
    @Test
    fun playingStateMapsToReady() {
        val f = MpvSessionFacts.of(MpvState(playing = true, buffering = false, started = true, loadId = 1, positionSecs = 12.5, durationSecs = 1440.0))
        assertEquals(Player.STATE_READY, f.playbackState)
        assertTrue(f.playWhenReady)
        assertTrue(f.playing)
        assertEquals(12_500L, f.positionMs)
        assertEquals(1_440_000L, f.durationMs)
        assertNull(f.error)
    }

    @Test
    fun pausedBufferingEndedAndError() {
        val paused = MpvSessionFacts.of(MpvState(playing = false, buffering = false, started = true, loadId = 1))
        assertEquals(Player.STATE_READY, paused.playbackState)
        assertFalse(paused.playWhenReady)
        val buffering = MpvSessionFacts.of(MpvState(playing = false, buffering = true, started = false, loadId = 2))
        assertEquals(Player.STATE_BUFFERING, buffering.playbackState)
        assertTrue(buffering.playWhenReady)
        assertFalse(buffering.playing)
        val ended = MpvSessionFacts.of(MpvState(ended = true, started = true, buffering = false, loadId = 1))
        assertEquals(Player.STATE_ENDED, ended.playbackState)
        val looping = MpvSessionFacts.of(MpvState(ended = true, loop = true, started = true, buffering = false, loadId = 1))
        assertEquals(Player.STATE_READY, looping.playbackState)
        val error = MpvSessionFacts.of(MpvState(error = "boom", loadId = 1))
        assertEquals(Player.STATE_IDLE, error.playbackState)
        assertEquals("boom", error.error)
        assertEquals(Player.STATE_IDLE, MpvSessionFacts.of(MpvState()).playbackState)
    }

    @Test
    fun presetChainsMatchDesktop() {
        assertTrue(ShaderPresets.chain("off").isEmpty())
        assertTrue(ShaderPresets.chain("unknown").isEmpty())
        assertEquals(listOf("FSR.glsl"), ShaderPresets.chain("fsr"))
        listOf("anime4k-a", "anime4k-aa", "anime4k-b", "anime4k-c").forEach { id ->
            val chain = ShaderPresets.chain(id)
            assertEquals("Anime4K_Clamp_Highlights.glsl", chain.first(), id)
            assertTrue(chain.last().startsWith("Anime4K_Upscale"), id)
        }
        assertEquals("00_Anime4K_Clamp_Highlights.glsl", ShaderPresets.targetNames("anime4k-a").first())
    }

    @Test
    fun everyShaderIsBundled() {
        val dir = File("src/main/assets/${ShaderPresets.ASSET_DIR}")
        if (!dir.isDirectory) return // test run from another working directory
        ShaderPresets.ALL_FILES.forEach { assertTrue(File(dir, it).isFile, "missing $it") }
    }
}
