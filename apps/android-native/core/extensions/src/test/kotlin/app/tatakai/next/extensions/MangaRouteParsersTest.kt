// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.MangaRouteQuery
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** BUG-099: Toko 4.x reads manga through `manga/chapters` / `manga/pages`, not through a `read` custom source. */
class MangaRouteParsersTest {
    @Test
    fun tokosMergedEntriesGiveOneChapterPerProviderAndKey() {
        val body = Json.parseToJsonElement(
            """{"result":[
                {"number":1,"title":"Dog & Chainsaw","volume":"1","sources":[
                    {"provider":"mangadex","chapterKey":"mangadex:abc","language":"en","scanlator":"Viz","releaseDate":"2020-01-05"},
                    {"chapterKey":"comick:xyz","language":"es-la"}]},
                {"number":"2","title":null,"volume":null,"sources":[{"provider":"mangadex","chapterKey":"mangadex:def","language":"en"},
                    {"provider":"mangadex","chapterKey":"mangadex:def","language":"en"}]}
            ]}""",
        )
        val list = ProviderParsers.mangaRouteChapters("tatakai.extension.toko", body)
        assertEquals(listOf("mangadex:abc", "comick:xyz", "mangadex:def"), list.map { it.key }, "duplicate keys dropped")
        assertEquals(listOf("mangadex", "comick", "mangadex"), list.map { it.provider }, "provider from the field or the key prefix")
        assertEquals(1.0, list[0].number)
        assertEquals(1, list[0].volume)
        assertEquals("Dog & Chainsaw", list[1].title, "the entry's title when the source has none")
        assertEquals("es", list[1].lang)
        assertEquals("Viz", list[0].group)
        assertEquals(1578182400000L, list[0].releasedAtMs)
        assertEquals(2.0, list[2].number)
    }

    @Test
    fun flatEntriesAndThePcServersProgressiveEnvelopeAreAccepted() {
        val pc = Json.parseToJsonElement("""{"chapters":[{"id":"ch-7","chapter":7,"lang":"es","group":"LatScans"}],"partial":false}""")
        val list = ProviderParsers.mangaRouteChapters("tatakai.extension.spanishpack", pc)
        assertEquals(1, list.size)
        assertEquals("tatakai.extension.spanishpack", list[0].provider, "no provider and no key prefix: the extension itself")
        assertEquals(7.0, list[0].number)
        assertEquals("LatScans", list[0].group)
        assertEquals(0, ProviderParsers.mangaRouteChapters("x", Json.parseToJsonElement("""{"error":"nope"}""")).size)
    }

    @Test
    fun paramsMatchTheDesktopCall() {
        val p = ProviderParsers.mangaRouteParams(MangaRouteQuery(anilistId = 105778, malId = null, titles = listOf("Chainsaw Man", "Chensō Man")))
        assertEquals(JsonPrimitive(105778), p["anilistId"])
        assertFalse("malId" in p)
        assertEquals(JsonPrimitive("Chainsaw Man"), p["title"])
        val pages = ProviderParsers.mangaPagesParams("mangadex:abc")
        assertEquals(JsonPrimitive("mangadex"), pages["provider"])
        assertEquals(JsonPrimitive("abc"), pages["providerChapterId"])
    }

    @Test
    fun manifestsThatReadMangaByRoute() {
        fun manifest(extra: String) = ManifestParser.parse(
            """{"id":"x","name":"X","type":"custom","version":"1","description":"","permissions":[]$extra}""",
        )
        // Toko 4.6.0 / Spanish pack: manga capability + the routes, no customSources.
        assertTrue(manifest(""","capabilities":["manga"],"apiServer":{"namespace":"x","routes":["sources","manga/chapters","manga/pages"]}""").readsMangaByRoute)
        assertTrue(manifest(""","capabilities":["manga"]""").readsMangaByRoute)
        // HentaiDB: a `read` custom source (custom/search + custom/info), not the route.
        assertFalse(
            manifest(
                ""","capabilities":["custom-source","manga"],"customSources":[{"id":"c","kind":"read","name":"C"}],
                   "apiServer":{"namespace":"x","routes":["custom/search","custom/read"]}""",
            ).readsMangaByRoute,
        )
        assertFalse(manifest("").readsMangaByRoute)
    }
}
