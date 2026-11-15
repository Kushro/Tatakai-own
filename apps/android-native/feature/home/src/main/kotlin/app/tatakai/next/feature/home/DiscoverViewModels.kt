// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.CalendarView
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.FollowedShow
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.RecFeedback
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.TasteProfile
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.time.DayOfWeek
import java.time.Instant
import java.time.LocalDate
import java.time.YearMonth
import java.time.ZoneId
import java.time.temporal.TemporalAdjusters

// Parity wave B (16 §3.1, worker r): ViewModels of the calendar (`Route.Calendar`) and "Para ti" (`Route.ForYou`).

/** One day column of the week view. */
data class CalendarDay(val date: LocalDate, val episodes: List<AiringEpisode>)

data class CalendarUiState(
    /** First day of the loaded range: Monday of the shown week (week / agenda), of the month grid (month) or the day itself (day). */
    val weekStart: LocalDate,
    /** 2026-11 UI refresh: Semana / Agenda / Mes / Hoy (default from Diseño y estilo; switching it on the screen is remembered there). */
    val view: CalendarView = CalendarView.WEEK,
    /** The day the view is about: the selected day of the week strip / month grid, the day of the day view. */
    val anchor: LocalDate = weekStart,
    val days: LoadState<List<CalendarDay>> = LoadState.Loading,
    val followed: List<FollowedShow> = emptyList(),
    val onlyFollowed: Boolean = false,
    /** AniList / MAL WATCHING titles, highlighted like the followed ones. */
    val watching: Set<MediaRef> = emptySet(),
) {
    fun isFollowed(ref: MediaRef): Boolean = followed.any { sameRef(it.ref, ref) }

    fun isTracked(ref: MediaRef): Boolean = isFollowed(ref) || watching.any { sameRef(it, ref) }

    /** What the screen draws, after the "Solo las que sigo" filter. */
    val visibleDays: LoadState<List<CalendarDay>>
        get() = when (val d = days) {
            is LoadState.Ok -> if (!onlyFollowed) d else LoadState.Ok(d.value.map { day -> day.copy(episodes = day.episodes.filter { isTracked(it.media.ref) }) })
            else -> d
        }
}

internal fun sameRef(a: MediaRef, b: MediaRef): Boolean =
    (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId)

/** Pure grouping of a week of episodes into local days (JVM-tested). */
internal object CalendarLogic {
    fun mondayOf(date: LocalDate): LocalDate = date.with(TemporalAdjusters.previousOrSame(DayOfWeek.MONDAY))

    /** Whole weeks (Monday..Sunday) covering [month]: 28, 35 or 42 days. */
    fun monthGrid(month: YearMonth): List<LocalDate> {
        val first = mondayOf(month.atDay(1))
        val last = month.atEndOfMonth().with(TemporalAdjusters.nextOrSame(DayOfWeek.SUNDAY))
        return generateSequence(first) { it.plusDays(1) }.takeWhile { !it.isAfter(last) }.toList()
    }

    /** First day and length of the range a [view] about [anchor] needs. */
    fun range(view: CalendarView, anchor: LocalDate): Pair<LocalDate, Int> = when (view) {
        CalendarView.WEEK, CalendarView.AGENDA -> mondayOf(anchor) to 7
        CalendarView.DAY -> anchor to 1
        CalendarView.MONTH -> monthGrid(YearMonth.from(anchor)).let { it.first() to it.size }
    }

    /** Previous / next page of a view: a week, a month (first day of it) or a day. */
    fun shift(view: CalendarView, anchor: LocalDate, delta: Long): LocalDate = when (view) {
        CalendarView.WEEK, CalendarView.AGENDA -> anchor.plusWeeks(delta)
        CalendarView.MONTH -> anchor.withDayOfMonth(1).plusMonths(delta)
        CalendarView.DAY -> anchor.plusDays(delta)
    }

    fun window(start: LocalDate, days: Int, zone: ZoneId): Pair<Long, Long> =
        start.atStartOfDay(zone).toEpochSecond() to start.plusDays(days.toLong()).atStartOfDay(zone).toEpochSecond()

    /** One line of the day view: an hour heading, an episode, or the "now" marker. */
    sealed interface DayLine {
        data class Hour(val hour: Int) : DayLine
        data class Ep(val episode: AiringEpisode) : DayLine
        data object Now : DayLine
    }

    /**
     * The day view: episodes in time order under their local hour, with [DayLine.Now] after everything that aired by [nowSec] (only
     * when now falls inside [day]).
     */
    fun dayLines(episodes: List<AiringEpisode>, day: LocalDate, zone: ZoneId, nowSec: Long): List<DayLine> {
        val sorted = episodes.sortedBy { it.airingAtEpochSec }
        val (from, to) = window(day, 1, zone)
        val showNow = nowSec in from until to
        val out = mutableListOf<DayLine>()
        var lastHour = -1
        var nowPlaced = !showNow
        for (ep in sorted) {
            if (!nowPlaced && ep.airingAtEpochSec > nowSec) {
                out += DayLine.Now
                nowPlaced = true
            }
            val hour = Instant.ofEpochSecond(ep.airingAtEpochSec).atZone(zone).hour
            if (hour != lastHour) {
                out += DayLine.Hour(hour)
                lastHour = hour
            }
            out += DayLine.Ep(ep)
        }
        if (!nowPlaced) out += DayLine.Now
        return out
    }

    fun weekWindow(weekStart: LocalDate, zone: ZoneId): Pair<Long, Long> {
        val from = weekStart.atStartOfDay(zone).toEpochSecond()
        val to = weekStart.plusDays(7).atStartOfDay(zone).toEpochSecond()
        return from to to
    }

    fun group(episodes: List<AiringEpisode>, weekStart: LocalDate, zone: ZoneId, days: Int = 7): List<CalendarDay> {
        val byDay = episodes.groupBy { Instant.ofEpochSecond(it.airingAtEpochSec).atZone(zone).toLocalDate() }
        return (0L until days.toLong()).map { i ->
            val day = weekStart.plusDays(i)
            CalendarDay(day, byDay[day].orEmpty().sortedBy { it.airingAtEpochSec })
        }
    }
}

class CalendarViewModel(
    private val airing: AiringRepository,
    lists: ListsRepository? = null,
    private val zone: () -> ZoneId = { ZoneId.systemDefault() },
    private val today: () -> LocalDate = { LocalDate.now() },
    /** 2026-11 UI refresh: default view from Diseño y estilo; a view picked on the screen is saved back there. */
    private val settings: SettingsRepository? = null,
) : ViewModel() {
    private val week = MutableStateFlow(
        (settings?.settings?.value?.look?.calendarView ?: CalendarView.WEEK).let { view ->
            val day = today()
            CalendarUiState(weekStart = CalendarLogic.range(view, day).first, view = view, anchor = day)
        },
    )

    private val watching = lists?.entries(null)?.map { list ->
        list.filter { it.status == ListStatus.WATCHING || it.status == ListStatus.REPEATING }.map { it.ref }.toSet()
    }?.catch { emit(emptySet()) } ?: flowOf(emptySet())

    val state: StateFlow<CalendarUiState> = combine(week, airing.followed, watching) { s, followed, w ->
        s.copy(followed = followed, watching = w)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), week.value)

    init {
        load()
        // Opening the calendar = the "Novedades de lo que sigues" of Home have been seen.
        viewModelScope.launch { runCatchingApp { airing.markVisited() } }
    }

    fun load() {
        val current = week.value
        val (start, count) = CalendarLogic.range(current.view, current.anchor)
        week.update { it.copy(weekStart = start, days = LoadState.Loading) }
        viewModelScope.launch {
            val z = zone()
            val (from, to) = CalendarLogic.window(start, count, z)
            val r = runCatchingApp { CalendarLogic.group(airing.schedule(from, to), start, z, count) }
            week.update { s ->
                if (s.weekStart != start || s.view != current.view) s
                else s.copy(days = r.fold(onSuccess = { LoadState.Ok(it) }, onFailure = { LoadState.Err(it.toAppError()) }))
            }
        }
    }

    fun shiftWeek(delta: Long) = shift(delta)

    /** Previous / next page of the current view (week, month or day). */
    fun shift(delta: Long) {
        week.update { it.copy(anchor = CalendarLogic.shift(it.view, it.anchor, delta)) }
        load()
    }

    /** Back to today in the current view. */
    fun goToday() {
        week.update { it.copy(anchor = today()) }
        load()
    }

    /** Selects a day of the loaded range (week strip / month grid); no reload while it stays inside the range. */
    fun selectDay(day: LocalDate) {
        val s = week.value
        val (start, count) = CalendarLogic.range(s.view, s.anchor)
        week.update { it.copy(anchor = day) }
        if (day.isBefore(start) || !day.isBefore(start.plusDays(count.toLong()))) load()
    }

    fun setView(view: CalendarView) {
        if (week.value.view == view) return
        week.update { it.copy(view = view) }
        load()
        settings?.let { repo ->
            viewModelScope.launch { runCatchingApp { repo.update { it.copy(look = it.look.copy(calendarView = view)) } } }
        }
    }

    fun setOnlyFollowed(only: Boolean) = week.update { it.copy(onlyFollowed = only) }

    fun toggleFollow(ep: AiringEpisode) {
        viewModelScope.launch {
            runCatchingApp {
                if (state.value.isFollowed(ep.media.ref)) airing.unfollow(ep.media.ref) else airing.follow(ep.media, remind = true)
            }
        }
    }

    fun unfollow(ref: MediaRef) {
        viewModelScope.launch { runCatchingApp { airing.unfollow(ref) } }
    }

    fun setReminder(ref: MediaRef, enabled: Boolean) {
        viewModelScope.launch { runCatchingApp { airing.setReminder(ref, enabled) } }
    }
}

enum class ForYouTab { LIST, ATLAS }

data class ForYouUiState(
    val tab: ForYouTab = ForYouTab.LIST,
    val items: LoadState<List<RecommendationItem>> = LoadState.Loading,
    val profile: TasteProfile? = null,
    val atlas: LoadState<List<AtlasPoint>>? = null,
    val dismissed: List<MediaRef> = emptyList(),
) {
    /** The list without what the user just dismissed (the repository drops them on the next computation). */
    val visible: LoadState<List<RecommendationItem>>
        get() = when (val i = items) {
            is LoadState.Ok -> LoadState.Ok(i.value.filter { item -> dismissed.none { sameRef(it, item.media.ref) } })
            else -> i
        }
}

class ForYouViewModel(private val recs: RecsRepository) : ViewModel() {
    private val local = MutableStateFlow(ForYouUiState())

    val state: StateFlow<ForYouUiState> = combine(local, recs.dismissed().catch { emit(emptyList()) }) { s, d -> s.copy(dismissed = d) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ForYouUiState())

    init {
        load()
    }

    fun load() {
        local.update { it.copy(items = LoadState.Loading) }
        viewModelScope.launch {
            val r = runCatchingApp { recs.forYou(LIMIT) }
            val profile = runCatchingApp { recs.profile() }.getOrNull()
            local.update { it.copy(items = r.fold(onSuccess = { v -> LoadState.Ok(v) }, onFailure = { e -> LoadState.Err(e.toAppError()) }), profile = profile) }
        }
    }

    fun selectTab(tab: ForYouTab) {
        local.update { it.copy(tab = tab) }
        if (tab == ForYouTab.ATLAS && (local.value.atlas == null || local.value.atlas is LoadState.Err)) loadAtlas()
    }

    fun loadAtlas() {
        local.update { it.copy(atlas = LoadState.Loading) }
        viewModelScope.launch {
            val r = runCatchingApp { recs.atlas() }
            local.update { it.copy(atlas = r.fold(onSuccess = { v -> LoadState.Ok(v) }, onFailure = { e -> LoadState.Err(e.toAppError()) })) }
        }
    }

    fun notInterested(ref: MediaRef) {
        viewModelScope.launch { runCatchingApp { recs.feedback(ref, RecFeedback.NOT_INTERESTED) } }
    }

    fun undo(ref: MediaRef) {
        viewModelScope.launch { runCatchingApp { recs.undoDismiss(ref) } }
    }

    private companion object {
        const val LIMIT = 30
    }
}
