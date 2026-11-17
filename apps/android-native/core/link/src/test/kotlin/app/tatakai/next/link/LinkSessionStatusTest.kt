// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import app.tatakai.next.model.ProcessingLocation
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.runTest
import java.nio.file.Files
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** 0.4.0: what the PC does with a session (`GET /v1/sessions/{id}`) and the "Procesar por defecto en" setting. */
class LinkSessionStatusTest {
    private fun decode(json: String) = LinkRepositoryImpl.statusOf(LinkHttp.json.decodeFromString(SessionStateDto.serializer(), json))

    @Test
    fun aNewPcSaysTheEffectiveMultiplierEncoderAndBitrate() {
        val s = decode(
            """{"state":"ready","generation":2,"instance_id":"i","profile":"p-anime4k-lsfg-1080-auto120","throttled":true,
            "output":{"width":1920,"height":1080,"fps":119.88},"window":{"first_segment":0,"bytes":1},"frontier_segment":40,"lead_secs":22.0,
            "upscale":"anime4k","anime4k":{"mode":"a","variant":"m"},"fg":"lsfg","encoder":"h264_vaapi","video_kbps":11000,
            "fg_effective":{"multiplier":5,"source_fps":23.976,"out_fps":119.88}}""",
        )
        assertEquals("p-anime4k-lsfg-1080-auto120", s.profileId)
        assertEquals(true, s.throttled)
        assertEquals(22.0, s.leadSecs)
        assertEquals(1920 to 1080, s.outputWidth to s.outputHeight)
        assertEquals(Triple("anime4k", "a", "m"), Triple(s.upscale, s.anime4kMode, s.anime4kVariant))
        assertEquals(Triple("lsfg", 5, 119.88), Triple(s.fg, s.fgMultiplier, s.outFps))
        assertEquals(23.976, s.sourceFps)
        assertEquals("h264_vaapi" to 11000, s.encoder to s.videoKbps)
    }

    @Test
    fun anOlderPcOnlyHasTheBasics() {
        val s = decode("""{"state":"preparing","generation":1,"instance_id":"i","profile":"p-none","throttled":false,"output":{"width":1920,"height":1080}}""")
        assertEquals("preparing", s.state)
        assertNull(s.upscale)
        assertNull(s.fg)
        assertNull(s.fgMultiplier)
        assertNull(s.encoder)
        assertNull(s.leadSecs)
        // 0.4.1 fields absent: not ended, nothing known about what the PC produced
        assertEquals(false, s.ended)
        assertNull(s.endSecs)
        assertNull(s.producedFromSecs)
        assertNull(s.producedToSecs)
    }

    @Test
    fun aNewPcSaysWhatItProducedAndWhereTheSourceEnds() {
        val s = decode(
            """{"state":"ready","generation":3,"instance_id":"i","profile":"p-none","throttled":false,
            "produced":{"from_secs":580.0,"to_secs":702.5},"ended":true,"end_secs":1418.37}""",
        )
        assertEquals(580.0 to 702.5, s.producedFromSecs to s.producedToSecs)
        assertEquals(true, s.ended)
        assertEquals(1418.37, s.endSecs)
        val info = app.tatakai.next.model.linkProcessingInfo("PC", "p-none", null, s)
        assertEquals(580.0 to 702.5, info.processedFromSecs to info.processedToSecs)
        // a failed session with the new reason parses like any other
        val failed = decode("""{"state":"failed","generation":3,"instance_id":"i","profile":"p-none","error":"engine_stalled","produced":{}}""")
        assertEquals("failed" to "engine_stalled", failed.state to failed.error)
        assertNull(failed.producedFromSecs)
    }

    @Test
    fun theDefaultLocationIsKeptWithThePairing() = runTest {
        val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        val base = Files.createTempDirectory("tatakai-processing-test").toFile()
        try {
            val vault = LinkKeyVault(base, AesGcmKeyCipher { secret })
            val pair = LinkCrypto.generateKeyPair()
            vault.save(StoredPairing("dev-1", "PC", "192.168.1.20", 47321, "ab".repeat(32)), LinkCrypto.privateKeyPem(pair.private), "x", "x")
            val repo = LinkRepositoryImpl(vault = vault, io = Dispatchers.Unconfined)
            assertEquals(ProcessingLocation.PHONE, repo.linkedDevice.value?.defaultProcessing)
            repo.setDefaultProcessing(ProcessingLocation.MIXED)
            assertEquals(ProcessingLocation.MIXED, repo.linkedDevice.value?.defaultProcessing)
            assertEquals(ProcessingLocation.MIXED, LinkRepositoryImpl(vault = vault, io = Dispatchers.Unconfined).linkedDevice.value?.defaultProcessing)
            repo.setDefaultProcessing(ProcessingLocation.PHONE)
            assertNull(vault.meta()?.defaultProcessing)
            repo.forget()
            assertNull(repo.linkedDevice.value)
        } finally {
            base.deleteRecursively() // temp dir created above by createTempDirectory
        }
    }
}
