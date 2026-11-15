// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** BUG-132: which Link profiles the phone's AVC decoder plays, and what replaces one it does not. */
class LinkProfileSupportTest {
    /** The moto g51's `c2.qti.avc.decoder` as seen in r7: up to 1920x1088, at most [maxFps] at 1080p. */
    private class FakeAvc(val maxW: Int = 1920, val maxH: Int = 1088, val maxFps: Double = 120.0) : VideoDecodeLimits {
        val asked = mutableListOf<Triple<Int, Int, Double?>>()
        override fun sizeSupported(width: Int, height: Int): Boolean {
            asked += Triple(width, height, null)
            return width <= maxW && height <= maxH
        }
        override fun sizeAndRateSupported(width: Int, height: Int, fps: Double): Boolean {
            asked += Triple(width, height, fps)
            return sizeSupported(width, height) && fps <= maxFps
        }
    }

    private fun p(id: String, w: Int, h: Int, upscale: Boolean = false, fg: Boolean = false, x: Int? = null, target: Int? = null) =
        LinkProfile(id, id, upscale, fg, w, h, "unknown", fgMultiplier = x, fgTargetFps = target)

    private val pcProfiles = listOf(
        p("p-none", 1920, 1080),
        p("p-anime4k-1080", 1920, 1080, upscale = true),
        p("p-anime4k-1440", 2560, 1440, upscale = true),
        p("p-anime4k-2160", 3840, 2160, upscale = true),
        p("p-lsfg-1080-x2", 1920, 1080, fg = true, x = 2),
        p("p-lsfg-1080-x5", 1920, 1080, fg = true, x = 5),
        p("p-lsfg-1080-auto120", 1920, 1080, fg = true, target = 120),
        p("p-anime4k-lsfg-1080-x3", 1920, 1080, upscale = true, fg = true, x = 3),
    )

    @Test
    fun theG51DoesNotPlay1440Or2160() {
        val bad = unsupportedLinkProfiles(pcProfiles, FakeAvc())
        assertEquals(mapOf("p-anime4k-1440" to ProfileSupport.TooLarge(1440), "p-anime4k-2160" to ProfileSupport.TooLarge(2160)), bad)
    }

    @Test
    fun frameGenerationCountsTheOutputFrameRate() {
        // x5 of a 24 fps source = 120 fps; a decoder that stops at 60 fps at 1080p cannot keep up
        val bad = unsupportedLinkProfiles(pcProfiles, FakeAvc(maxFps = 60.0))
        assertEquals(ProfileSupport.TooFast(1080, 120), bad["p-lsfg-1080-x5"])
        assertEquals(ProfileSupport.TooFast(1080, 120), bad["p-lsfg-1080-auto120"])
        assertEquals(ProfileSupport.TooFast(1080, 72), bad["p-anime4k-lsfg-1080-x3"])
        assertNull(bad["p-lsfg-1080-x2"]) // 48 fps
        assertNull(bad["p-none"])
    }

    @Test
    fun theFrameRateIsReadFromTheIdWhenThePcDoesNotSendIt() {
        assertEquals(96, linkProfileFps(p("p-lsfg-1080-x4", 1920, 1080, fg = true)))
        assertEquals(48, linkProfileFps(p("p-anime4k-1080-fg2x", 1920, 1080, upscale = true, fg = true)))
        assertEquals(120, linkProfileFps(p("p-anime4k-lsfg-1080-auto120", 1920, 1080, upscale = true, fg = true)))
        assertEquals(LINK_SOURCE_FPS, linkProfileFps(p("p-anime4k-1440", 2560, 1440, upscale = true)))
    }

    @Test
    fun withoutDetectionEverythingCounts() {
        assertTrue(unsupportedLinkProfiles(pcProfiles, null).isEmpty())
    }

    @Test
    fun aProfileTheDecoderRejectedStaysOut() {
        // a 1440p-capable phone whose decoder still failed with 1440 in this player
        val big = FakeAvc(maxW = 4096, maxH = 2176)
        val bad = unsupportedLinkProfiles(pcProfiles, big, rejected = setOf("p-anime4k-1440"))
        assertEquals(mapOf("p-anime4k-1440" to ProfileSupport.Rejected), bad)
        assertEquals("p-anime4k-1080", playableLinkProfile("p-anime4k-1440", pcProfiles, big, rejected = setOf("p-anime4k-1440")))
    }

    @Test
    fun anUnplayableProfileFallsBackToTheClosestPlayableOne() {
        val g51 = FakeAvc()
        assertEquals("p-anime4k-1080", playableLinkProfile("p-anime4k-1440", pcProfiles, g51))
        assertEquals("p-anime4k-1080", playableLinkProfile("p-anime4k-2160", pcProfiles, g51))
        assertEquals("p-anime4k-1080", playableLinkProfile("p-anime4k-1080", pcProfiles, g51))
        // same kind at a lower frame rate: LSFG x5 -> x2 on a 60 fps decoder
        assertEquals("p-lsfg-1080-x2", playableLinkProfile("p-lsfg-1080-x5", pcProfiles, FakeAvc(maxFps = 60.0)))
        // nothing of that kind: the original video
        assertEquals("p-none", playableLinkProfile("p-anime4k-lsfg-1080-x3", pcProfiles, FakeAvc(maxFps = 60.0)))
    }

    @Test
    fun thePcDefaultIsCheckedToo() {
        assertEquals("p-anime4k-1080", playableLinkProfile(null, pcProfiles, FakeAvc(), defaultProfile = "p-anime4k-1440"))
        assertEquals("p-none", playableLinkProfile(null, pcProfiles, FakeAvc(), defaultProfile = "p-none"))
        // an id the PC list does not have is left to the PC
        assertEquals("p-new", playableLinkProfile("p-new", pcProfiles, FakeAvc()))
        assertNull(playableLinkProfile("p-anime4k-1440", listOf(pcProfiles[2]), FakeAvc()))
    }
}
