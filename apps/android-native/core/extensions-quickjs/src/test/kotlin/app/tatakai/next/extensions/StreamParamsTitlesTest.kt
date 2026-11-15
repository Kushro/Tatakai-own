package app.tatakai.next.extensions

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.withAltTitles
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals

// BUG-143: the titles a `/streams` call gets. With AniList the order is the one Toko 4.6.2 was tested with; a Kitsu fallback
// carries the English / romaji titles kept from AniList (or its synonyms) as alt titles instead of only "Temppal: Item no Chikara".
class StreamParamsTitlesTest {
    private fun media(title: String, titles: Map<String, String>, provider: String) = Media(
        MediaRef(anilistId = 212888), MediaKind.ANIME, title, titles, null, null, null, null, emptyList(), 12, null, null, null, null,
        provider,
    )

    private fun titlesOf(m: Media): List<String> =
        ExtensionRepositoryImpl.streamParams(m, 4)["titles"]!!.jsonArray.map { it.jsonPrimitive.content }

    @Test
    fun aniListOrderIsUnchanged() {
        val m = media(
            "Overgeared",
            linkedMapOf("romaji" to "Tempal: Item no Chikara", "english" to "Overgeared", "native" to "テムパル～アイテムの力～"),
            "anilist",
        )
        assertEquals(listOf("Overgeared", "Tempal: Item no Chikara", "テムパル～アイテムの力～"), titlesOf(m))
    }

    @Test
    fun kitsuFallbackCarriesTheEnglishTitleKnownBefore() {
        val kitsu = media(
            "Temppal: Item no Chikara",
            linkedMapOf("romaji" to "Temppal: Item no Chikara", "native" to "テムパル～アイテムの力～"),
            "kitsu",
        ).withAltTitles(listOf("Overgeared", "Tempal: Item no Chikara", "テムパル～アイテムの力～", "overgeared", " "))
        assertEquals(
            listOf("Temppal: Item no Chikara", "テムパル～アイテムの力～", "Overgeared", "Tempal: Item no Chikara"),
            titlesOf(kitsu),
        )
        val params = ExtensionRepositoryImpl.streamParams(kitsu, 4)
        assertEquals(212888, params["anilistId"]!!.jsonPrimitive.content.toInt())
        assertEquals("4", params["episode"]!!.jsonPrimitive.content)
    }
}
