// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.FollowedShow
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.RecFeedback
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.TasteProfile
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.io.IOException
import java.time.LocalDate
import java.time.ZoneId
import java.time.ZoneOffset
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

// F6 (parity wave B tests, 16 §5 r.2 / r.3): CalendarViewModel and ForYouViewModel over fake AiringRepository / RecsRepository,
// with the injected zone and "today" of the calendar.
@OptIn(ExperimentalCoroutinesApi::class)
class DiscoverViewModelsTest {
    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val utc: ZoneId = ZoneOffset.UTC
    private val wednesday: LocalDate = LocalDate.of(2026, 11, 11)
    private val monday: LocalDate = LocalDate.of(2026, 11, 9)

    private fun media(id: Int) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "Anime $id", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = emptyList(), episodes = 12, status = "RELEASING", season = null, year = 2026, score = 80,
        sourceProvider = "fake",
    )

    private fun epochOf(day: LocalDate, hour: Int) = day.atTime(hour, 0).toEpochSecond(ZoneOffset.UTC)

    private class FakeAiring(var episodes: List<AiringEpisode> = emptyList(), var fail: Boolean = false) : AiringRepository {
        override val followed = MutableStateFlow<List<FollowedShow>>(emptyList())
        val windows = mutableListOf<Pair<Long, Long>>()
        var visited = 0
        val reminders = mutableListOf<Pair<MediaRef, Boolean>>()

        override fun isFollowed(ref: MediaRef): Flow<Boolean> = followed.map { list -> list.any { it.ref == ref } }
        override suspend fun follow(media: Media, remind: Boolean) {
            followed.value = followed.value + FollowedShow(media.ref, media.title, followedAtMs = 1, remind = remind)
        }
        override suspend fun unfollow(ref: MediaRef) {
            followed.value = followed.value.filterNot { it.ref == ref }
        }
        override suspend fun setReminder(ref: MediaRef, enabled: Boolean) {
            reminders += ref to enabled
        }
        override suspend fun schedule(fromEpochSec: Long, toEpochSec: Long): List<AiringEpisode> {
            windows += fromEpochSec to toEpochSec
            if (fail) throw IOException("offline")
            return episodes.filter { it.airingAtEpochSec in fromEpochSec until toEpochSec }
        }
        override suspend fun newSinceLastVisit(): List<AiringEpisode> = emptyList()
        override suspend fun markVisited() {
            visited++
        }
    }

    private fun TestScope.subscribe(flow: Flow<*>) {
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { flow.collect {} }
    }

    // ---- calendar ---------------------------------------------------------------------------------------------------------------

    @Test
    fun calendarLoadsTheWeekOfTodayAndMarksVisited() = runTest {
        val tue = AiringEpisode(media(1), 5, epochOf(monday.plusDays(1), 18))
        val sun = AiringEpisode(media(2), 3, epochOf(monday.plusDays(6), 9))
        val nextWeek = AiringEpisode(media(3), 1, epochOf(monday.plusDays(8), 9))
        val airing = FakeAiring(listOf(sun, tue, nextWeek))
        val vm = CalendarViewModel(airing, lists = null, zone = { utc }, today = { wednesday })
        subscribe(vm.state)

        assertEquals(monday, vm.state.value.weekStart)
        assertEquals(1, airing.visited)
        assertEquals(epochOf(monday, 0) to epochOf(monday.plusDays(7), 0), airing.windows.single())
        val days = assertIs<LoadState.Ok<List<CalendarDay>>>(vm.state.value.days).value
        assertEquals(7, days.size)
        assertEquals(listOf(5), days[1].episodes.map { it.episode })
        assertEquals(listOf(3), days[6].episodes.map { it.episode })
        assertTrue(days.flatMap { it.episodes }.none { it.media.ref.anilistId == 3 })
    }

    @Test
    fun shiftWeekLoadsTheNextWindow() = runTest {
        val nextWeek = AiringEpisode(media(3), 1, epochOf(monday.plusDays(8), 9))
        val airing = FakeAiring(listOf(nextWeek))
        val vm = CalendarViewModel(airing, null, zone = { utc }, today = { wednesday })
        subscribe(vm.state)
        vm.shiftWeek(1)
        assertEquals(monday.plusWeeks(1), vm.state.value.weekStart)
        assertEquals(epochOf(monday.plusDays(7), 0), airing.windows.last().first)
        val days = assertIs<LoadState.Ok<List<CalendarDay>>>(vm.state.value.days).value
        assertEquals(listOf(1), days[1].episodes.map { it.episode })
    }

    @Test
    fun calendarErrorIsMapped() = runTest {
        val vm = CalendarViewModel(FakeAiring(fail = true), null, zone = { utc }, today = { wednesday })
        subscribe(vm.state)
        assertEquals(LoadState.Err(AppError.Offline), vm.state.value.days)
    }

    @Test
    fun followToggleAndOnlyFollowedFilter() = runTest {
        val a = AiringEpisode(media(1), 5, epochOf(monday, 18))
        val b = AiringEpisode(media(2), 7, epochOf(monday, 20))
        val airing = FakeAiring(listOf(a, b))
        val vm = CalendarViewModel(airing, null, zone = { utc }, today = { wednesday })
        subscribe(vm.state)

        vm.toggleFollow(a)
        assertTrue(vm.state.value.isFollowed(a.media.ref))
        assertTrue(airing.followed.value.single().remind, "following from the calendar turns the reminder on")

        vm.setOnlyFollowed(true)
        val visible = assertIs<LoadState.Ok<List<CalendarDay>>>(vm.state.value.visibleDays).value
        assertEquals(listOf(5), visible[0].episodes.map { it.episode })

        vm.toggleFollow(a)
        assertTrue(airing.followed.value.isEmpty())
        val none = assertIs<LoadState.Ok<List<CalendarDay>>>(vm.state.value.visibleDays).value
        assertTrue(none[0].episodes.isEmpty())

        vm.setReminder(b.media.ref, false)
        assertEquals(listOf(b.media.ref to false), airing.reminders)
    }

    @Test
    fun sameRefMatchesByEitherId() {
        assertTrue(sameRef(MediaRef(anilistId = 1), MediaRef(anilistId = 1, malId = 9)))
        assertTrue(sameRef(MediaRef(malId = 9), MediaRef(anilistId = 2, malId = 9)))
        assertTrue(!sameRef(MediaRef(), MediaRef()))
    }

    // ---- for you ----------------------------------------------------------------------------------------------------------------

    private class FakeRecs(var items: List<RecommendationItem>, var failAtlas: Boolean = false) : RecsRepository {
        val dismissedFlow = MutableStateFlow<List<MediaRef>>(emptyList())
        var atlasCalls = 0
        var profileValue: TasteProfile? = TasteProfile(mapOf("Fantasy" to 1f), emptyMap(), emptyMap(), 80, 2)
        override suspend fun profile(): TasteProfile? = profileValue
        override suspend fun forYou(limit: Int): List<RecommendationItem> = items.take(limit)
        override suspend fun feedback(ref: MediaRef, feedback: RecFeedback) {
            dismissedFlow.value = dismissedFlow.value + ref
        }
        override fun dismissed(): Flow<List<MediaRef>> = dismissedFlow
        override suspend fun undoDismiss(ref: MediaRef) {
            dismissedFlow.value = dismissedFlow.value - ref
        }
        override suspend fun atlas(): List<AtlasPoint> {
            atlasCalls++
            if (failAtlas) throw IOException("offline")
            return items.mapIndexed { i, it -> AtlasPoint(it.media, i / 10f, i / 10f, 0, recommended = true) }
        }
    }

    @Test
    fun forYouLoadsItemsAndHidesDismissedUntilUndo() = runTest {
        val recs = FakeRecs(listOf(RecommendationItem(media(1), 0.9f, emptyList()), RecommendationItem(media(2), 0.5f, emptyList())))
        val vm = ForYouViewModel(recs)
        subscribe(vm.state)
        assertEquals(2, assertIs<LoadState.Ok<List<RecommendationItem>>>(vm.state.value.visible).value.size)
        assertEquals(2, vm.state.value.profile?.sampleSize)

        vm.notInterested(MediaRef(anilistId = 1))
        assertEquals(listOf(2), assertIs<LoadState.Ok<List<RecommendationItem>>>(vm.state.value.visible).value.map { it.media.ref.anilistId })
        vm.undo(MediaRef(anilistId = 1))
        assertEquals(2, assertIs<LoadState.Ok<List<RecommendationItem>>>(vm.state.value.visible).value.size)
    }

    @Test
    fun atlasLoadsOnceAndRetriesAfterAnError() = runTest {
        val recs = FakeRecs(listOf(RecommendationItem(media(1), 0.9f, emptyList())), failAtlas = true)
        val vm = ForYouViewModel(recs)
        subscribe(vm.state)
        assertNull(vm.state.value.atlas)
        vm.selectTab(ForYouTab.ATLAS)
        assertEquals(LoadState.Err(AppError.Offline), vm.state.value.atlas)
        recs.failAtlas = false
        vm.selectTab(ForYouTab.LIST)
        vm.selectTab(ForYouTab.ATLAS)
        assertEquals(2, recs.atlasCalls, "an errored atlas is retried when the tab is opened again")
        assertEquals(1, assertIs<LoadState.Ok<List<AtlasPoint>>>(vm.state.value.atlas).value.size)
        vm.selectTab(ForYouTab.LIST)
        vm.selectTab(ForYouTab.ATLAS)
        assertEquals(2, recs.atlasCalls, "a loaded atlas is kept")
    }
}
