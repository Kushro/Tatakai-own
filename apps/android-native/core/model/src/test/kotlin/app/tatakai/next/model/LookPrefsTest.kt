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
        assertEquals(EpisodeView.LIST, LookPrefs().episodeView)
        assertEquals(listOf("standard", "editorial"), PageStyle.entries.map { it.id })
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

    @Test
    fun `episode list and page style per preset`() {
        LookPreset.entries.forEach { assertEquals(EpisodeView.LIST, LookPrefs.of(it).episodeView, it.id) }
        assertEquals(PageStyle.EDITORIAL, LookPrefs.of(LookPreset.CINEMA).pageStyle)
        listOf(LookPreset.TATAKAI, LookPreset.DENSE, LookPreset.CALM).forEach { assertEquals(PageStyle.STANDARD, LookPrefs.of(it).pageStyle) }
        assertEquals(PageStyle.STANDARD, json.decodeFromString(LookPrefs.serializer(), """{"pageStyle":"nope"}""").pageStyle)
        assertEquals(EpisodeView.LIST, json.decodeFromString(LookPrefs.serializer(), """{"episodeView":"nope"}""").episodeView)
        assertNull(LookPrefs.of(LookPreset.CINEMA).copy(pageStyle = PageStyle.STANDARD).matchingPreset())
        assertEquals(LookPrefs.CURRENT_REV, LookPrefs().rev)
        assertEquals(LookPreset.CINEMA, LookPrefs.of(LookPreset.CINEMA).copy(rev = 0).sanitized().matchingPreset())
    }

    @Test
    fun `card size ids presets and lenient reading`() {
        assertEquals(listOf("small", "medium", "large", "xl"), CardSize.entries.map { it.id })
        assertEquals(CardSize.MEDIUM, LookPrefs().cardSize)
        assertEquals(CardSize.MEDIUM, LookPrefs.of(LookPreset.TATAKAI).cardSize)
        assertEquals(CardSize.LARGE, LookPrefs.of(LookPreset.CINEMA).cardSize)
        assertEquals(CardSize.SMALL, LookPrefs.of(LookPreset.DENSE).cardSize)
        assertEquals(CardSize.MEDIUM, LookPrefs.of(LookPreset.CALM).cardSize)
        assertEquals(listOf(false, false, true, true), CardSize.entries.map { it.isWide })
        assertEquals(CardSize.MEDIUM, json.decodeFromString(LookPrefs.serializer(), """{"cardSize":"huge"}""").cardSize)
        assertEquals(CardSize.XL, json.decodeFromString(LookPrefs.serializer(), """{"cardSize":"xl"}""").cardSize)
        assertNull(LookPrefs.of(LookPreset.CINEMA).copy(cardSize = CardSize.MEDIUM).matchingPreset())
    }

    @Test
    fun `header layout ids default and lenient reading`() {
        assertEquals(listOf("line", "facts", "nextup", "facts_nextup"), HeaderLayout.entries.map { it.id })
        assertEquals(HeaderLayout.FACTS_NEXT_UP, LookPrefs().headerLayout)
        assertEquals(listOf(false, false, true, true), HeaderLayout.entries.map { it.hasCard })
        assertEquals(listOf(false, true, false, true), HeaderLayout.entries.map { it.usesFacts })
        LookPreset.entries.forEach { assertEquals(HeaderLayout.FACTS_NEXT_UP, LookPrefs.of(it).headerLayout, it.id) }
        assertEquals(HeaderLayout.FACTS_NEXT_UP, json.decodeFromString(LookPrefs.serializer(), """{"headerLayout":"stacked"}""").headerLayout)
        val old = json.decodeFromString(LookPrefs.serializer(), """{"preset":"cinema","cardStyle":"minimal","cardSize":"large","railStyle":"slim","glass":true,"heroIntervalSecs":12,"pageStyle":"editorial"}""")
        assertEquals(HeaderLayout.FACTS_NEXT_UP, old.headerLayout)
        assertEquals(LookPreset.CINEMA, old.matchingPreset())
        val line = json.decodeFromString(LookPrefs.serializer(), """{"headerLayout":"line"}""")
        assertEquals(HeaderLayout.LINE, line.headerLayout)
        assertEquals(HeaderLayout.LINE, json.decodeFromString(LookPrefs.serializer(), json.encodeToString(LookPrefs.serializer(), line)).headerLayout)
        // no preset sets it: another composition keeps the preset
        assertEquals(LookPreset.CINEMA, LookPrefs.of(LookPreset.CINEMA).copy(headerLayout = HeaderLayout.LINE).matchingPreset())
    }
}
