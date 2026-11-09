// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §3.4, worker t): custom provider answers (extension host routes and PC server ops).
class ProviderParsersTest {
    private fun j(s: String) = Json.parseToJsonElement(s)

    @Test
    fun homeSectionsAndBareLists() {
        val rows = ProviderParsers.homeRows(
            "ext", "src", "Fallback",
            j("""{"sections":[{"title":"Latest","items":[{"id":"1","title":"One","image":"https://i/1.jpg"},{"id":"1","title":"Dup"}]},{"title":"Empty","items":[]}]}"""),
        )
        assertEquals(1, rows.size)
        assertEquals("Latest", rows[0].title)
        assertEquals(listOf("1"), rows[0].items.map { it.id })
        assertEquals("https://i/1.jpg", rows[0].items[0].poster)
        val bare = ProviderParsers.homeRows("ext", "src", "Fallback", j("""[{"url":"/a","name":"A","poster":"p"}]"""))
        assertEquals("Fallback", bare.single().title)
        assertEquals("/a", bare.single().items.single().id)
    }

    @Test
    fun searchCardsAndPaging() {
        val body = j("""{"results":[{"id":5,"title":"Five"},{"title":"No id"}],"hasNextPage":true}""")
        val cards = ProviderParsers.cards(body)
        assertEquals(listOf("5", "No id"), cards.map { it.id })
        assertTrue(ProviderParsers.hasNextPage(body))
    }

    @Test
    fun infoWithEntriesOrSeparateEpisodes() {
        val item = ProviderParsers.info(
            "x", "watch",
            j("""{"id":"show","title":"Show","image":"https://i/s.jpg","meta":{"genres":"Action, Drama","status":"Ongoing"},
                 "entries":[{"id":"e1","label":"Pilot","number":1},{"id":"e2","number":"2"}]}"""),
        )
        assertEquals("Show", item?.title)
        assertEquals(listOf("Action", "Drama"), item?.genres)
        assertEquals("Ongoing", item?.status)
        assertEquals(listOf(1.0, 2.0), item?.entries?.map { it.number })
        assertEquals("Pilot", item?.entries?.first()?.title)
        val split = ProviderParsers.info("x", "read", j("""{"name":"Manga"}"""), j("""{"chapters":[{"id":"c1","chapter":1.5}]}"""))
        assertEquals("read", split?.entries?.single()?.kind)
        assertEquals(1.5, split?.entries?.single()?.number)
        assertNull(ProviderParsers.info("x", "watch", j("""{"nothing":true}""")))
    }

    @Test
    fun pagesSortedAndNovelParagraphs() {
        val pages = ProviderParsers.pages(
            "read",
            j("""{"pages":[{"pageNumber":2,"imageUrl":"https://i/2.jpg"},{"pageNumber":1,"imageUrl":"https://i/1.jpg","headers":{"Referer":"https://m/"}},{"imageUrl":"ftp://bad"}]}"""),
        )
        assertEquals(listOf("https://i/1.jpg", "https://i/2.jpg"), pages.map { it.url })
        assertEquals(listOf(0, 1), pages.map { it.index })
        val novel = ProviderParsers.pages("novel", j("""{"content":"<p>First line</p><p>Second</p>"}"""))
        assertEquals(listOf("First line", "Second"), novel.map { it.url })
    }
}
