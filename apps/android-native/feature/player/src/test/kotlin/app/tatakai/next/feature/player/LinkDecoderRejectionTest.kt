// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.VideoDecodeLimits
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** BUG-132: a Link profile the phone's decoder rejects is an error of the phone, goes back to the previous profile and is never retried. */
class LinkDecoderRejectionTest {
    /** The moto g51's `c2.qti.avc.decoder`: up to 1920x1088. */
    private object G51 : VideoDecodeLimits {
        override fun sizeSupported(width: Int, height: Int) = width <= 1920 && height <= 1088
        override fun sizeAndRateSupported(width: Int, height: Int, fps: Double) = sizeSupported(width, height) && fps <= 120
    }

    /** A phone whose decoder announces 4K but still fails at init (the limits are not always right). */
    private object Optimistic : VideoDecodeLimits {
        override fun sizeSupported(width: Int, height: Int) = true
        override fun sizeAndRateSupported(width: Int, height: Int, fps: Double) = true
    }

    private fun p(id: String, w: Int, h: Int, upscale: Boolean = false) = LinkProfile(id, id, upscale, false, w, h, "unknown")

    private val profiles = listOf(
        p("p-none", 1920, 1080),
        p("p-anime4k-1080", 1920, 1080, upscale = true),
        p("p-anime4k-1440", 2560, 1440, upscale = true),
        p("p-anime4k-2160", 3840, 2160, upscale = true),
    )

    @Test
    fun decoderInitFailuresAreThePhones() {
        // r7: DecoderInitializationException c2.qti.avc.decoder [2560, 1440] -> ERROR_CODE_DECODER_INIT_FAILED
        assertTrue(isDecoderRejection(4001))
        assertTrue(isDecoderRejection(4003))
        assertTrue(isDecoderRejection(4005))
        assertFalse(isDecoderRejection(4002)) // decoding failed mid-stream: not about the profile
        assertFalse(isDecoderRejection(2001)) // network
    }

    @Test
    fun aProfileTheLimitsRuleOutIsRejectedAtOnceOthersAfterTheSilentRetry() {
        assertTrue(linkDecoderRejected(4001, knownUnplayable = true, silentRetrySpent = false))
        assertFalse(linkDecoderRejected(4001, knownUnplayable = false, silentRetrySpent = false))
        assertTrue(linkDecoderRejected(4001, knownUnplayable = false, silentRetrySpent = true))
        assertFalse(linkDecoderRejected(2001, knownUnplayable = true, silentRetrySpent = true))
    }

    @Test
    fun aRejectedChangeGoesBackToThePreviousProfile() {
        val rejected = setOf("p-anime4k-1440")
        assertEquals("p-none", linkProfileAfterRejection("p-anime4k-1440", "p-none", profiles, Optimistic, rejected, "p-none"))
        // without a previous profile (the session started with it): the closest playable one
        assertEquals("p-anime4k-1080", linkProfileAfterRejection("p-anime4k-1440", null, profiles, G51, rejected, "p-none"))
        // the previous one was rejected too: not that one
        assertEquals(
            "p-anime4k-1080",
            linkProfileAfterRejection("p-anime4k-2160", "p-anime4k-1440", profiles, Optimistic, setOf("p-anime4k-1440", "p-anime4k-2160"), "p-none"),
        )
    }

    @Test
    fun withoutAnythingPlayableTheErrorScreenStays() {
        val onlyBig = listOf(p("p-anime4k-1440", 2560, 1440, upscale = true))
        assertNull(linkProfileAfterRejection("p-anime4k-1440", null, onlyBig, G51, setOf("p-anime4k-1440"), "p-anime4k-1440"))
        // the PC's profile list never loaded: the original video
        assertEquals("p-none", linkProfileAfterRejection("p-anime4k-1440", null, emptyList(), null, setOf("p-anime4k-1440"), null))
    }

    @Test
    fun retryNeverRepeatsARejectedProfile() {
        val rejected = setOf("p-anime4k-1440")
        // r7: Reintentar asked for p-anime4k-1440 again and failed the same way
        assertEquals("p-anime4k-1080", linkRetryProfile("p-anime4k-1440", profiles, G51, rejected, "p-none"))
        assertEquals("p-none", linkRetryProfile("p-anime4k-1440", emptyList(), null, rejected, null))
        // a profile that was not rejected is retried as is (a PC error, a lost connection)
        assertEquals("p-anime4k-1080", linkRetryProfile("p-anime4k-1080", profiles, G51, rejected, "p-none"))
        assertNull(linkRetryProfile(null, profiles, G51, rejected, "p-none"))
    }
}
