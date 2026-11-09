// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtensionHomeItem
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals

/** Parity wave A (worker o): `custom/search` card parsing behind ExtensionRepository.searchExtension. */
class ExtensionSearchTest {
    private fun obj(s: String) = Json.parseToJsonElement(s) as JsonObject

    @Test
    fun cardsFromResultsItemsOrBareArray() {
        val results = obj("""{"namespace":"x","results":[{"id":"a1","title":" Frieren ","image":"https://i/x.jpg"},{"id":7,"title":"Seven"},{"title":""},{"id":"z"}]}""")
        assertEquals(
            listOf(ExtensionHomeItem("a1", "Frieren", "https://i/x.jpg"), ExtensionHomeItem("7", "Seven", null)),
            ExtensionSearch.cards(results),
        )
        assertEquals(listOf("B"), ExtensionSearch.cards(obj("""{"items":[{"title":"B","poster":"p"}]}""")).map { it.title })
        assertEquals("B", ExtensionSearch.cards(obj("""{"result":[{"title":"B"}]}""")).single().id)
        assertEquals(emptyList(), ExtensionSearch.cards(obj("""{"results":null}""")))
    }

    @Test
    fun mergeKeepsSourceOrderAndDropsDuplicateTitles() {
        val merged = ExtensionSearch.merge(
            listOf(
                listOf(ExtensionHomeItem("1", "Frieren", null), ExtensionHomeItem("2", "Dandadan", null)),
                listOf(ExtensionHomeItem("9", "frieren", "p"), ExtensionHomeItem("3", "Mushishi", null)),
            ),
        )
        assertEquals(listOf("1", "2", "3"), merged.map { it.id })
    }
}
