// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.PhoneFrameGenAvailability
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.ProcessingLocation.MIXED
import app.tatakai.next.model.ProcessingLocation.PC
import app.tatakai.next.model.ProcessingLocation.PHONE
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.0: which "Procesar en" options can be chosen and why not; the engine of each location; where a player starts. */
class ProcessingOptionsTest {
    private val ready = ProcessingFacts(
        current = PHONE,
        paired = true,
        reachable = true,
        external = false,
        sendable = SendableState.Sendable,
        hasOriginal = true,
        mpvAvailable = true,
        phoneFrameGen = PhoneFrameGenAvailability(eligible = true),
        clientFgAllowed = true,
        capabilitiesKnown = true,
        mixedProfile = "p-anime4k-1080",
    )

    private fun blocks(f: ProcessingFacts) = processingOptions(f).associate { it.location to it.block }

    @Test
    fun everythingAvailableWithAPairedPcAndAWebSource() {
        assertEquals(mapOf(PHONE to null, PC to null, MIXED to null), blocks(ready))
        assertTrue(processingOptions(ready).all { it.available })
    }

    @Test
    fun theLinkReasonsComeFirstThenTheSourceThenMixedOwnOnes() {
        assertEquals(ProcessingBlock.NOT_PAIRED, blocks(ready.copy(paired = false, reachable = false))[PC])
        assertEquals(ProcessingBlock.PC_OFFLINE, blocks(ready.copy(reachable = false))[MIXED])
        assertEquals(ProcessingBlock.LOCAL_FILE, blocks(ready.copy(sendable = SendableState.LocalFile))[PC])
        assertEquals(ProcessingBlock.SOURCE_NOT_SENDABLE, blocks(ready.copy(sendable = SendableState.NotSendable))[MIXED])
        assertEquals(ProcessingBlock.NO_SOURCE_YET, blocks(ready.copy(sendable = SendableState.NothingYet))[PC])
        assertEquals(ProcessingBlock.EXTERNAL_PLAYER, blocks(ready.copy(external = true))[PC])

        assertEquals(ProcessingBlock.MPV_UNAVAILABLE, blocks(ready.copy(mpvAvailable = false))[MIXED])
        val noFg = processingOptions(ready.copy(phoneFrameGen = PhoneFrameGenAvailability(false, "vulkan"))).first { it.location == MIXED }
        assertEquals(ProcessingBlock.PHONE_FG_UNAVAILABLE to "vulkan", noFg.block to noFg.detail)
        assertEquals(ProcessingBlock.PHONE_FG_UNAVAILABLE, blocks(ready.copy(phoneFrameGen = PhoneFrameGenAvailability.UNBOUND))[MIXED], "nothing bound yet")
        assertEquals(ProcessingBlock.PC_SUPPRESSES_PHONE_FG, blocks(ready.copy(clientFgAllowed = false))[MIXED])
        assertEquals(ProcessingBlock.PC_NO_UPSCALE_PROFILE, blocks(ready.copy(mixedProfile = null))[MIXED])
        // capabilities not known yet: not a reason (the switch asks for them)
        assertNull(blocks(ready.copy(clientFgAllowed = null, capabilitiesKnown = false, mixedProfile = null))[MIXED])
        // strict PC is not affected by the Mixto reasons
        assertNull(blocks(ready.copy(mpvAvailable = false, clientFgAllowed = false, mixedProfile = null))[PC])
    }

    @Test
    fun fromThePcTheSourceDoesNotMatterButThePhoneNeedsAnOriginal() {
        val onPc = ready.copy(current = PC, sendable = SendableState.NothingYet)
        assertEquals(mapOf(PHONE to null, PC to null, MIXED to null), blocks(onPc), "PC <-> Mixto only changes the profile")
        assertEquals(ProcessingBlock.NO_ORIGINAL_SOURCE, blocks(onPc.copy(hasOriginal = false))[PHONE])
        // the current location is always available, even when the PC went away
        assertNull(blocks(onPc.copy(reachable = false))[PC])
        assertEquals(ProcessingBlock.PC_OFFLINE, blocks(onPc.copy(reachable = false))[MIXED])
    }

    @Test
    fun mixedPlaysOnMpvAndStrictPcOnMedia3() {
        assertEquals(PlayerEngine.MPV, engineFor(MIXED, PlayerEngine.MEDIA3, mpvAvailable = true))
        assertEquals(PlayerEngine.MEDIA3, engineFor(PC, PlayerEngine.MPV, mpvAvailable = true))
        assertEquals(PlayerEngine.MPV, engineFor(PC, PlayerEngine.MPV, mpvAvailable = true, pcOnMpv = true))
        assertEquals(PlayerEngine.MEDIA3, engineFor(PC, PlayerEngine.MEDIA3, mpvAvailable = true, pcOnMpv = true))
        assertEquals(PlayerEngine.MPV, engineFor(PHONE, PlayerEngine.MPV, mpvAvailable = true))
        assertEquals(PlayerEngine.MEDIA3, engineFor(PHONE, PlayerEngine.MEDIA3, mpvAvailable = true))
    }

    @Test
    fun whereAPlayerStarts() {
        assertEquals(PC, startLocation(remoteSession = true, carried = PHONE, default = MIXED, paired = true), "a hand-off plays from the PC")
        assertEquals(MIXED, startLocation(false, carried = MIXED, default = PHONE, paired = true), "the previous episode's choice")
        assertEquals(PC, startLocation(false, carried = null, default = PC, paired = true), "Settings default")
        assertEquals(PHONE, startLocation(false, carried = null, default = PC, paired = false))
        assertEquals(PHONE, startLocation(false, carried = null, default = null, paired = true))

        ProcessingCarry.put("anilist:1", PC)
        assertNull(ProcessingCarry.take("anilist:2"), "another title does not inherit it")
        assertNull(ProcessingCarry.take("anilist:1"), "taken once (or dropped)")
        ProcessingCarry.put("anilist:1", MIXED)
        assertEquals(MIXED, ProcessingCarry.take("anilist:1"))
    }
}
