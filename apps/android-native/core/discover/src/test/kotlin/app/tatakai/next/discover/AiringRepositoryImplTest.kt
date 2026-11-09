// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AiringEpisode
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AiringRepositoryImplTest {
    private val day = 86_400L
    private val now = 1_800_000_000L

    private fun repo(meta: FakeMetadata, reminders: Boolean = true, clockSec: () -> Long = { now }) =
        AiringRepositoryImpl(meta, null, Files.createTempDirectory("airing").toFile(), { reminders }, clock = { clockSec() * 1000 })

    @Test
    fun scheduleKeepsTheWindowSortedAndDeduped() = runTest {
        val a = AiringEpisode(media(1), 3, now + 100)
        val b = AiringEpisode(media(2), 1, now + 50)
        val meta = FakeMetadata(listOf(a, b, a, AiringEpisode(media(3), 9, now + day * 10)))
        val list = repo(meta).schedule(now, now + day * 7)
        assertEquals(listOf(b, a), list)
    }

    @Test
    fun followPersistsAndNewSinceLastVisitOnlyTracked() = runTest {
        val dir = Files.createTempDirectory("airing").toFile()
        val meta = FakeMetadata(
            listOf(
                AiringEpisode(media(1), 4, now - day * 2),
                AiringEpisode(media(2), 7, now - day),
                AiringEpisode(media(1), 5, now - 60),
                AiringEpisode(media(1), 3, now - day * 20),
            ),
        )
        var t = now
        val r = AiringRepositoryImpl(meta, null, dir, { true }, clock = { t * 1000 })
        assertTrue(r.newSinceLastVisit().isEmpty(), "nothing followed")
        r.follow(media(1))
        assertTrue(r.isFollowed(media(1).ref).first())
        assertEquals(listOf(5, 4), r.newSinceLastVisit().map { it.episode }, "default 7 days, newest first")
        r.markVisited()
        t = now + 10
        assertTrue(r.newSinceLastVisit().isEmpty(), "after the visit only newer episodes")
        // reload from disk
        val again = AiringRepositoryImpl(meta, null, dir, { true }, clock = { t * 1000 })
        assertEquals(listOf(media(1).ref), again.followed.value.map { it.ref })
        again.unfollow(media(1).ref)
        assertTrue(again.followed.value.isEmpty())
    }

    @Test
    fun remindersHonourTheFlagAndAdvance() = runTest {
        var t = now
        val meta = FakeMetadata(listOf(AiringEpisode(media(1), 2, now - 60), AiringEpisode(media(1), 3, now + 600)))
        val dir = Files.createTempDirectory("airing").toFile()
        var enabled = true
        val r = AiringRepositoryImpl(meta, null, dir, { enabled }, clock = { t * 1000 })
        r.follow(media(1))
        assertEquals(listOf(2), r.dueReminders().map { it.episode })
        assertTrue(r.dueReminders().isEmpty(), "already reminded")
        t = now + 900
        enabled = false
        assertTrue(r.dueReminders().isEmpty(), "reminders off")
        enabled = true
        t = now + 1800
        r.setReminder(media(1).ref, false)
        assertTrue(r.dueReminders().isEmpty(), "per-show reminder off")
    }

    @Test
    fun lookbackIsBounded() {
        assertEquals(now - 14 * day, AiringLogic.lookbackStart(now - 100 * day, now, 14 * day, 7 * day))
        assertEquals(now - 7 * day, AiringLogic.lookbackStart(null, now, 14 * day, 7 * day))
        assertEquals(now - 10, AiringLogic.lookbackStart(now - 10, now, 14 * day, 7 * day))
    }
}
