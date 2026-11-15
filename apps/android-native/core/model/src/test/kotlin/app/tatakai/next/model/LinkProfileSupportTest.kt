// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** BUG-132 / BUG-135: which Link profiles the phone's AVC decoder plays, and what replaces one it does not. */
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
    fun aFrameRateAboveTheDeclaredOneNeverBlocks() {
        // BUG-135: the g51 declares about 1080p60 but plays LSFG auto120 (state=3 in r4-r7); x5 / auto120 = 120 fps, x3 = 72 fps
        val g51 = FakeAvc(maxFps = 60.0)
        val bad = unsupportedLinkProfiles(pcProfiles, g51)
        assertEquals(mapOf("p-anime4k-1440" to ProfileSupport.TooLarge(1440), "p-anime4k-2160" to ProfileSupport.TooLarge(2160)), bad)
        // only a note on the row
        assertEquals(
            setOf("p-lsfg-1080-x5", "p-lsfg-1080-auto120", "p-anime4k-lsfg-1080-x3"),
            linkProfilesMayDropFrames(pcProfiles, g51),
        )
        // 48 fps fits; a size the decoder refuses gets its own reason, not the note
        assertFalse(linkProfileMayDropFrames(pcProfiles[4], g51))
        assertFalse(linkProfileMayDropFrames(pcProfiles[2], g51))
        assertTrue(linkProfilesMayDropFrames(pcProfiles, null).isEmpty())
    }

    @Test
    fun theSizeCheckAloneDecides1080pAt120Fps() {
        val auto120 = p("p-anime4k-lsfg-1080-auto120", 1920, 1080, upscale = true, fg = true, target = 120)
        val g51 = FakeAvc(maxFps = 60.0)
        assertEquals(ProfileSupport.Supported, linkProfileSupport(auto120, g51))
        assertTrue(g51.asked.all { it.third == null }) // never asked about the rate
        assertTrue(linkProfileMayDropFrames(auto120, g51))
    }

    @Test
    fun aPreferredProfileIsNeverReplacedForItsFrameRate() {
        // BUG-135: r8 replaced p-anime4k-lsfg-1080-auto120 with ...-x2 on the g51
        val g51 = FakeAvc(maxFps = 60.0)
        assertEquals("p-lsfg-1080-x5", playableLinkProfile("p-lsfg-1080-x5", pcProfiles, g51))
        assertEquals("p-lsfg-1080-auto120", playableLinkProfile("p-lsfg-1080-auto120", pcProfiles, g51))
        assertEquals("p-anime4k-lsfg-1080-x3", playableLinkProfile("p-anime4k-lsfg-1080-x3", pcProfiles, g51))
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
        // a rejected profile goes to the same kind at a lower size or frame rate (x5 and auto120 rejected -> x2)
        assertEquals(
            "p-lsfg-1080-x2",
            playableLinkProfile("p-lsfg-1080-x5", pcProfiles, g51, rejected = setOf("p-lsfg-1080-x5", "p-lsfg-1080-auto120")),
        )
        // nothing of that kind: the original video
        assertEquals("p-none", playableLinkProfile("p-anime4k-lsfg-1080-x3", pcProfiles, g51, rejected = setOf("p-anime4k-lsfg-1080-x3")))
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
