// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.ProviderEntry
import app.tatakai.next.model.ProviderEntryRef
import app.tatakai.next.model.ProviderItem
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runTest
import java.io.IOException
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ProviderResumeTest {
    private val historyId = "ext:server:pc1/kai:src:item-9"

    private fun row(episode: Int, progressSec: Int, completed: Boolean, animeId: String = historyId) = HistoryEntry(
        animeId = animeId, title = "T", poster = null, episodeId = "$animeId?ep=$episode", episode = episode, progressSec = progressSec,
        durationSec = 1400, completed = completed, watchedAt = Instant.ofEpochSecond(100), anilistId = null, malId = null,
    )

    private val item = ProviderItem(
        id = "item-9",
        title = "Show",
        entries = listOf(
            ProviderEntry("e1", "watch", number = 1.0),
            ProviderEntry("e2", "watch", number = 2.0),
            ProviderEntry("e3", "watch", number = 3.0),
        ),
    )

    private class FakeRepo(private val info: ProviderItem?, private val fail: Boolean = false) : ExtensionRepository {
        val asked = mutableListOf<Triple<String, String, String>>()
        override val installed = MutableStateFlow<List<InstalledExtension>>(emptyList())
        override suspend fun installFromUri(uri: String): Result<InstalledExtension> = Result.failure(IllegalStateException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> = emptyList()
        override suspend fun providerInfo(extensionId: String, sourceId: String, itemId: String): ProviderItem? {
            asked += Triple(extensionId, sourceId, itemId)
            if (fail) throw IOException("offline")
            return info
        }
    }

    @Test
    fun parsesHistoryIdsWithColonsInTheExtensionId() {
        assertEquals(Triple("server:pc1/kai", "src", "item-9"), parseProviderHistoryId(historyId))
        assertEquals(Triple("kai", "src", "x"), parseProviderHistoryId("ext:kai:src:x"))
        assertNull(parseProviderHistoryId("12345"))
        assertNull(parseProviderHistoryId("ext:kai:src"))
        assertNull(parseProviderHistoryId("ext::src:x"))
    }

    @Test
    fun anUnfinishedEpisodeResumesAtItsPosition() {
        val args = providerResumeArgs("kai", "src", item, row(2, 300, completed = false))!!
        assertEquals(2, args.episode)
        assertEquals(300_000L, args.startMs)
        assertEquals(ProviderEntryRef("kai", "src", "item-9", "e2"), args.provider)
    }

    @Test
    fun aFinishedEpisodeContinuesWithTheNextOneFromTheStart() {
        val args = providerResumeArgs("kai", "src", item, row(2, 1390, completed = true))!!
        assertEquals(3, args.episode)
        assertEquals(0L, args.startMs)
        assertEquals("e3", args.provider?.entryId)
    }

    @Test
    fun theLastFinishedOrAMissingEpisodeHasNoTarget() {
        assertNull(providerResumeArgs("kai", "src", item, row(3, 1390, completed = true)))
        assertNull(providerResumeArgs("kai", "src", item, row(7, 10, completed = false)))
    }

    @Test
    fun resumerReadsTheTitlePageOnce() = runTest {
        val repo = FakeRepo(item)
        val args = ProviderResumer(repo).resume(row(1, 42, completed = false))
        assertEquals(listOf(Triple("server:pc1/kai", "src", "item-9")), repo.asked)
        assertEquals(1, args?.episode)
        assertEquals(42_000L, args?.startMs)
        assertEquals("server:pc1/kai", args?.provider?.extensionId)
    }

    @Test
    fun resumerFallsBackWhenThePageCannotBeRead() = runTest {
        assertNull(ProviderResumer(FakeRepo(null)).resume(row(1, 42, completed = false)))
        assertNull(ProviderResumer(FakeRepo(item, fail = true)).resume(row(1, 42, completed = false)))
        val notProvider = FakeRepo(item)
        assertNull(ProviderResumer(notProvider).resume(row(1, 42, completed = false, animeId = "21")))
        assertEquals(0, notProvider.asked.size)
    }
}
