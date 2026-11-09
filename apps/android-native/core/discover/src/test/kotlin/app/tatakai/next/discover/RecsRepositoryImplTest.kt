// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.RecFeedback
import app.tatakai.next.model.Recommendation
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

// F6 (parity wave B tests, 16 §5 r.3): RecsRepositoryImpl over fake metadata / history, TemporaryFolder store and an injected clock.
class RecsRepositoryImplTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private class RecsMetadata(
        val byId: Map<Int, Media>,
        val similar: Map<Int, List<Media>> = emptyMap(),
        val trending: List<Media> = emptyList(),
    ) : MetadataRepository {
        var mediaCalls = 0
        var feedCalls = 0
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
            feedCalls++
            return if (feed == FeedKind.TRENDING) FeedPage(trending, page, false) else FeedPage(emptyList(), page, false)
        }
        override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
            mediaCalls++
            return ref.anilistId?.let(byId::get)
        }
        override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
        override suspend fun recommendations(ref: MediaRef): List<Recommendation> =
            similar[ref.anilistId].orEmpty().map { Recommendation(it, rating = 10) }
    }

    private class FakeHistory(initial: List<HistoryEntry>) : HistoryRepository {
        val entries = MutableStateFlow(initial)
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = entries
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = MutableStateFlow(emptyMap())
    }

    private fun watched(id: Int, completed: Boolean = true) = HistoryEntry(
        animeId = id.toString(), title = "T$id", poster = null, episodeId = "$id?ep=1", episode = 1, progressSec = 1300, durationSec = 1400,
        completed = completed, watchedAt = Instant.ofEpochSecond(1_700_000_000L + id), anilistId = id, malId = null,
    )

    private val seen1 = media(1, listOf("Fantasy", "Adventure"), 2023, 88)
    private val similarToSeen = media(10, listOf("Fantasy", "Adventure"), 2024, 85)
    private val romance = media(11, listOf("Romance"), 2001, 60)

    private var now = 1_000_000L

    private fun repo(meta: MetadataRepository, history: HistoryRepository, dir: File = tmp.root.resolve("recs")) =
        RecsRepositoryImpl(metadata = meta, history = history, lists = null, storeDir = dir, clock = { now })

    private fun defaultMeta() = RecsMetadata(
        byId = mapOf(1 to seen1),
        similar = mapOf(1 to listOf(similarToSeen)),
        trending = listOf(romance, seen1),
    )

    @Test
    fun noHistoryMeansNoProfileAndNoRecommendations() = runTest {
        val r = repo(defaultMeta(), FakeHistory(emptyList()))
        assertNull(r.profile())
        assertTrue(r.forYou().isEmpty())
    }

    @Test
    fun forYouExcludesSeenTitlesAndExplainsSimilarOnes() = runTest {
        val r = repo(defaultMeta(), FakeHistory(listOf(watched(1))))
        val profile = assertNotNull(r.profile())
        assertEquals(1, profile.sampleSize)
        val items = r.forYou()
        val ids = items.map { it.media.ref.anilistId }
        assertFalse(1 in ids, "already watched titles are never recommended")
        assertEquals(10, ids.first(), "the similar fantasy title ranks above the unrelated romance")
        assertTrue(11 in ids)
        assertTrue(items.first().reasons.any { it.label == seen1.title })
    }

    @Test
    fun dismissedTitlesPersistAcrossInstancesAndCanBeUndone() = runTest {
        val dir = tmp.root.resolve("recs")
        val history = FakeHistory(listOf(watched(1)))
        val first = repo(defaultMeta(), history, dir)
        first.feedback(similarToSeen.ref, RecFeedback.NOT_INTERESTED)
        first.feedback(similarToSeen.ref, RecFeedback.NOT_INTERESTED)
        assertEquals(1, first.dismissed().first().size, "dismissing twice keeps one entry")
        assertFalse(10 in first.forYou().map { it.media.ref.anilistId })
        assertTrue(File(dir, "dismissed.json").isFile)

        val reopened = repo(defaultMeta(), history, dir)
        assertEquals(listOf(10), reopened.dismissed().first().map { it.anilistId })
        assertFalse(10 in reopened.forYou().map { it.media.ref.anilistId })

        reopened.undoDismiss(MediaRef(anilistId = 10))
        assertTrue(reopened.dismissed().first().isEmpty())
        assertTrue(10 in reopened.forYou().map { it.media.ref.anilistId })
        val again = repo(defaultMeta(), history, dir)
        assertTrue(again.dismissed().first().isEmpty(), "undo is persisted too")
    }

    @Test
    fun computationIsCachedForThirtyMinutes() = runTest {
        val meta = defaultMeta()
        val r = repo(meta, FakeHistory(listOf(watched(1))))
        r.forYou()
        val feeds = meta.feedCalls
        assertTrue(feeds > 0)
        now += 29L * 60_000
        r.forYou()
        r.profile()
        assertEquals(feeds, meta.feedCalls, "within the cache window nothing is fetched again")
        now += 2L * 60_000
        r.forYou()
        assertTrue(meta.feedCalls > feeds, "after 30 min the candidates are recomputed")
    }

    @Test
    fun wipeClearsDismissedAndTheStoreDir() = runTest {
        val dir = tmp.root.resolve("recs")
        val r = repo(defaultMeta(), FakeHistory(listOf(watched(1))), dir)
        r.feedback(similarToSeen.ref, RecFeedback.NOT_INTERESTED)
        assertTrue(dir.exists())
        r.wipe()
        assertTrue(r.dismissed().first().isEmpty())
        assertFalse(dir.exists())
    }

    @Test
    fun atlasHasTheSeenAndTheRecommendedTitles() = runTest {
        val r = repo(defaultMeta(), FakeHistory(listOf(watched(1))))
        val atlas = r.atlas()
        assertTrue(atlas.any { !it.recommended && it.media.ref.anilistId == 1 })
        assertTrue(atlas.any { it.recommended && it.media.ref.anilistId == 10 })
        assertTrue(atlas.all { it.x in 0f..1f && it.y in 0f..1f })
    }

    @Test
    fun metadataFailuresDoNotBreakTheFeed() = runTest {
        val failing = object : MetadataRepository by defaultMeta() {
            override suspend fun recommendations(ref: MediaRef): List<Recommendation> = throw java.io.IOException("down")
        }
        val r = repo(failing, FakeHistory(listOf(watched(1))))
        val ids = r.forYou().map { it.media.ref.anilistId }
        assertEquals(listOf(11), ids, "the feeds still provide candidates")
    }

    // ---- JsonFileStore -----------------------------------------------------------------------------------------------------------

    @Test
    fun jsonFileStoreRoundTripsAndFallsBackOnCorruption() = runTest {
        val file = tmp.root.resolve("store/values.json")
        val store = JsonFileStore(file, ListSerializer(String.serializer())) { listOf("default") }
        assertEquals(listOf("default"), store.read(), "missing file = default")
        store.write(listOf("a", "b"))
        assertEquals(listOf("a", "b"), store.read())
        assertEquals(listOf("a", "b"), JsonFileStore(file, ListSerializer(String.serializer())) { emptyList() }.readBlocking())
        assertFalse(File(file.path + ".tmp").exists(), "the temp file is renamed away")
        file.writeText("{not json")
        assertEquals(listOf("default"), store.read(), "corrupt file = default")
    }
}
