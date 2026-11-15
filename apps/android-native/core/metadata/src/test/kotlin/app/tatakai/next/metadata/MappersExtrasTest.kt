// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 2026-11 UI refresh fields left out of the shared fixture comparison (format, episode summary / runtime / rating). */
class MappersExtrasTest {
    @Test
    fun anilistFormat() {
        val page = Mappers.aniListPage(Fixtures.element("anilist-trending-page.json"))
        assertEquals("TV", page.items.first().format)
    }

    @Test
    fun aniZipEpisodeExtras() {
        val eps = Mappers.aniZipEpisodes(Fixtures.element("anizip-frieren.json") as JsonObject)
        val first = eps.first()
        assertEquals(26, first.runtimeMin)
        assertEquals(8.03, first.rating)
        assertTrue(first.summary!!.startsWith("The world celebrates the Demon King's defeat"))
    }

    @Test
    fun aniZipRatingAsNumberAndBadValues() {
        val raw = Json.parseToJsonElement(
            """{"episodes":{
              "1":{"title":{"en":"A"},"rating":7.456,"length":24,"summary":"x`s"},
              "2":{"title":{"en":"B"},"rating":"n/a","runtime":0},
              "3":{"title":{"en":"C"},"rating":"11"}}}""",
        ) as JsonObject
        val eps = Mappers.aniZipEpisodes(raw)
        assertEquals(7.46, eps[0].rating)
        assertEquals(24, eps[0].runtimeMin)
        assertEquals("x's", eps[0].summary)
        assertNull(eps[1].rating)
        assertNull(eps[1].runtimeMin)
        assertNull(eps[2].rating)
    }

    @Test
    fun kitsuEpisodeExtras() {
        val eps = Mappers.kitsuEpisodes(Fixtures.element("kitsu-episodes.json") as JsonObject)
        val first = eps.first()
        assertEquals(24, first.runtimeMin)
        assertNull(first.rating)
        // The trailing "(Source: Crunchyroll)" is cleaned like the series synopsis.
        assertTrue(first.summary!!.endsWith("what will Frieren the mage do next?"))
    }
}
