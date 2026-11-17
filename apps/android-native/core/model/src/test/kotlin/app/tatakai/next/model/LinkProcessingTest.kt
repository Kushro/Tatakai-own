// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.0: the in-player profile list (grouped, filtered), the "Mixto" profile and what the PC does. */
class LinkProcessingTest {
    private fun p(
        id: String,
        h: Int = 1080,
        upscale: Boolean = false,
        fg: Boolean = false,
        sustain: String = "unknown",
        mult: Int? = null,
        target: Int? = null,
        fgKind: String = if (fg) "lsfg" else "none",
    ) = LinkProfile(id, id, upscale, fg, h * 16 / 9, h, sustain, mult, target, fgKind = fgKind, encoder = "h264_vaapi")

    private val catalog = listOf(
        p("p-none", sustain = "true"),
        p("p-anime4k-1080", upscale = true, sustain = "true"),
        p("p-anime4k-1440", 1440, upscale = true),
        p("p-anime4k-2160", 2160, upscale = true, sustain = "false"),
        p("p-anime4k-1080-fg2x", upscale = true, fg = true, fgKind = "frc_amf"),
        p("p-lsfg-1080-x2", fg = true, mult = 2),
        p("p-lsfg-1080-auto120", fg = true, target = 120),
        p("p-anime4k-lsfg-1080-x3", upscale = true, fg = true, mult = 3),
    )

    /** A g51-like decoder: nothing above 1080p. */
    private val upTo1080 = object : VideoDecodeLimits {
        override fun sizeSupported(width: Int, height: Int) = height <= 1080
        override fun sizeAndRateSupported(width: Int, height: Int, fps: Double) = height <= 1080 && fps <= 60
    }

    @Test
    fun profilesAreGroupedHidingTheUnsustainableAndDisablingTheUndecodable() {
        val groups = groupedLinkProfiles(catalog, upTo1080)
        assertEquals(
            listOf(LinkProfileGroupKind.NONE, LinkProfileGroupKind.UPSCALE, LinkProfileGroupKind.FRAME_GEN, LinkProfileGroupKind.UPSCALE_FRAME_GEN),
            groups.map { it.kind },
        )
        val upscale = groups.first { it.kind == LinkProfileGroupKind.UPSCALE }.rows
        assertEquals(listOf("p-anime4k-1080", "p-anime4k-1440"), upscale.map { it.profile.id }, "2160 was measured as not sustainable")
        assertEquals(ProfileSupport.TooLarge(1440), upscale[1].support)
        assertTrue(!upscale[1].enabled && upscale[0].enabled)
        assertEquals(listOf("p-lsfg-1080-x2", "p-lsfg-1080-auto120"), groups.first { it.kind == LinkProfileGroupKind.FRAME_GEN }.rows.map { it.profile.id })
        // 120 fps is above what this decoder declares: selectable, with the note
        assertTrue(groups.first { it.kind == LinkProfileGroupKind.FRAME_GEN }.rows.first { it.profile.id == "p-lsfg-1080-auto120" }.mayDropFrames)
        assertEquals(listOf("p-anime4k-1080-fg2x", "p-anime4k-lsfg-1080-x3"), groups.last().rows.map { it.profile.id })
    }

    @Test
    fun thePlayingProfileStaysVisibleAndMixedOnlyListsUpscaleProfiles() {
        val withCurrent = groupedLinkProfiles(catalog, null, current = "p-anime4k-2160")
        assertTrue(withCurrent.flatMap { it.rows }.any { it.profile.id == "p-anime4k-2160" })
        val mixed = groupedLinkProfiles(catalog, upTo1080, upscaleOnly = true)
        assertEquals(listOf(LinkProfileGroupKind.NONE, LinkProfileGroupKind.UPSCALE), mixed.map { it.kind })
        assertEquals(emptyList(), groupedLinkProfiles(emptyList(), null))
    }

    @Test
    fun mixedUsesAnUpscaleOnlyProfileThePhoneDecodes() {
        // the upscale part of the PC profile in use
        assertEquals("p-anime4k-1080", mixedLinkProfile("p-anime4k-lsfg-1080-x3", catalog, upTo1080))
        assertEquals("p-anime4k-1440", mixedLinkProfile("p-anime4k-1440", catalog, null))
        // 1440 is too large for this phone: the closest to 1080
        assertEquals("p-anime4k-1080", mixedLinkProfile("p-anime4k-1440", catalog, upTo1080))
        // nothing upscaled in use: aim at 1080p
        assertEquals("p-anime4k-1080", mixedLinkProfile("p-lsfg-1080-x2", catalog, null))
        assertEquals("p-anime4k-1080", mixedLinkProfile(null, catalog, null))
        // rejected by the decoder, or measured unsustainable: never chosen
        assertEquals("p-anime4k-1440", mixedLinkProfile(null, catalog, null, rejected = setOf("p-anime4k-1080")))
        assertNull(mixedLinkProfile(null, catalog.filter { !it.upscale || it.frameGeneration }, null), "no upscale-only profile: Mixed is blocked")
        assertNull(mixedLinkProfile(null, listOf(p("p-anime4k-2160", 2160, upscale = true, sustain = "false")), null))
    }

    @Test
    fun whatThePcDoesFromCapabilitiesAndStatus() {
        val auto = catalog.first { it.id == "p-lsfg-1080-auto120" }
        val before = linkProcessingInfo("Octavio-PC", auto.id, auto, null)
        assertEquals(LinkFrameGen.LsfgAuto(120, null, null), before.frameGen, "an older PC only says auto")
        assertEquals(LinkUpscale.None, before.upscale)
        assertEquals("h264_vaapi", before.encoder)
        val status = LinkSessionStatus(
            state = "ready", profileId = auto.id, throttled = false, leadSecs = 2.0, outputWidth = 1920, outputHeight = 1080,
            upscale = "none", fg = "lsfg", fgMultiplier = 5, sourceFps = 23.976, outFps = 119.88, encoder = "h264_nvenc", videoKbps = 11000,
        )
        val after = linkProcessingInfo("Octavio-PC", auto.id, auto, status)
        assertEquals(LinkFrameGen.LsfgAuto(120, 5, 119.88), after.frameGen)
        assertEquals("h264_nvenc" to 11000, after.encoder to after.videoKbps)
        assertTrue(after.fallingBehind, "2 s ready and not held back")
        assertTrue(!after.copy(throttled = true).fallingBehind)

        val a4k = catalog.first { it.id == "p-anime4k-lsfg-1080-x3" }
        val info = linkProcessingInfo(null, a4k.id, a4k, status.copy(profileId = a4k.id, upscale = "anime4k", anime4kMode = "a", anime4kVariant = "m", fgMultiplier = 3, outFps = 71.928))
        assertEquals(LinkUpscale.Anime4k("a", "m"), info.upscale)
        assertEquals(LinkFrameGen.Lsfg(3, 71.928), info.frameGen)
        val amf = catalog.first { it.id == "p-anime4k-1080-fg2x" }
        assertEquals(LinkFrameGen.FrcAmf, linkProcessingInfo(null, amf.id, amf, null).frameGen)
        assertEquals(LinkFrameGen.None, linkProcessingInfo(null, "p-none", catalog[0], null).frameGen)
        // the PC lowered the profile: the status's profile wins over the requested one
        val degraded = linkProcessingInfo(null, "p-anime4k-1440", null, LinkSessionStatus(state = "ready", profileId = "p-none", degradedFrom = "p-anime4k-1440"))
        assertEquals("p-none" to "p-anime4k-1440", degraded.profileId to degraded.degradedFrom)
    }
}
