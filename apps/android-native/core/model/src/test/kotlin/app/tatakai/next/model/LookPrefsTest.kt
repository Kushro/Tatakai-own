// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** Same cases as the desktop `look.rs` tests: ids, presets, lenient reading. */
class LookPrefsTest {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    @Test
    fun `ids are the desktop ones and round trip`() {
        assertEquals(listOf("spotlight", "carousel", "rows"), HomeLayout.entries.map { it.id })
        assertEquals(listOf("classic", "detailed", "minimal"), CardStyle.entries.map { it.id })
        assertEquals(listOf("week", "agenda", "month", "day"), CalendarView.entries.map { it.id })
        assertEquals(listOf("tatakai", "cinema", "dense", "calm"), LookPreset.entries.map { it.id })
        CalendarView.entries.forEach { assertEquals(it, lookChoiceOf<CalendarView>(it.id)) }
        assertNull(lookChoiceOf<Motion>("nope"))
        assertEquals(EpisodeView.CARDS, LookPrefs().episodeView)
    }

    @Test
    fun `preset values match the desktop`() {
        val t = LookPrefs.of(LookPreset.TATAKAI)
        assertEquals(LookPrefs(), t)
        assertEquals(9, t.heroIntervalSecs)
        val c = LookPrefs.of(LookPreset.CINEMA)
        assertEquals(CardStyle.MINIMAL, c.cardStyle)
        assertEquals(RailStyle.SLIM, c.railStyle)
        assertEquals(true, c.glass)
        assertEquals(12, c.heroIntervalSecs)
        assertEquals(HomeLayout.SPOTLIGHT, c.homeLayout)
        val d = LookPrefs.of(LookPreset.DENSE)
        assertEquals(
            listOf(HomeLayout.ROWS, CardStyle.CLASSIC, RailStyle.SLIM, Density.COMPACT, Motion.SUBTLE),
            listOf(d.homeLayout, d.cardStyle, d.railStyle, d.density, d.motion),
        )
        assertEquals(listOf(EpisodeView.LIST, RelationsView.LIST, CalendarView.AGENDA), listOf(d.episodeView, d.relationsView, d.calendarView))
        assertEquals(true, d.showGreeting)
        val calm = LookPrefs.of(LookPreset.CALM)
        assertEquals(HomeLayout.CAROUSEL, calm.homeLayout)
        assertEquals(false, calm.heroAutoplay)
        assertNull(calm.heroAutoplayMs())
        assertEquals(CalendarView.DAY, calm.calendarView)
        assertEquals(EpisodeView.LIST, calm.episodeView)
        assertEquals(Motion.SUBTLE, calm.motion)
    }

    @Test
    fun `presets are recognised until a field changes`() {
        LookPreset.entries.forEach { assertEquals(it, LookPrefs.of(it).matchingPreset()) }
        val custom = LookPrefs.of(LookPreset.CINEMA).copy(density = Density.COMPACT)
        assertNull(custom.matchingPreset())
        assertEquals(LookPreset.TATAKAI, LookPrefs().matchingPreset())
    }

    @Test
    fun `unknown ids read as defaults and bad values are repaired`() {
        val s = json.decodeFromString(
            LookPrefs.serializer(),
            """{"homeLayout":"nope","cardStyle":"minimal","heroIntervalSecs":200,"future":1}""",
        )
        assertEquals(HomeLayout.SPOTLIGHT, s.homeLayout)
        assertEquals(CardStyle.MINIMAL, s.cardStyle)
        assertEquals(LookPrefs.HERO_INTERVAL_MAX, s.sanitized().heroIntervalSecs)
        assertEquals(LookPrefs(), json.decodeFromString(LookPrefs.serializer(), "{}"))
        assertEquals(9_000L, LookPrefs().heroAutoplayMs())
        val round = json.decodeFromString(LookPrefs.serializer(), json.encodeToString(LookPrefs.serializer(), LookPrefs.of(LookPreset.DENSE)))
        assertEquals(LookPrefs.of(LookPreset.DENSE), round)
        assert(json.encodeToString(LookPrefs.serializer(), LookPrefs()).contains("\"cardStyle\":\"detailed\""))
    }
}
