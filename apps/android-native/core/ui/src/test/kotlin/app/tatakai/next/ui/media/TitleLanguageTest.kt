// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.media

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.TitleLanguage
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class TitleLanguageTest {

    private val media = Media(
        ref = MediaRef(anilistId = 154587),
        kind = MediaKind.ANIME,
        title = "Sousou no Frieren",
        titles = mapOf("romaji" to "Sousou no Frieren", "english" to "Frieren: Beyond Journey's End", "native" to "葬送のフリーレン"),
        cover = null,
        banner = null,
        color = null,
        synopsis = null,
        genres = emptyList(),
        episodes = 28,
        status = null,
        season = null,
        year = 2023,
        score = 91,
        sourceProvider = "anilist",
    )

    @Test
    fun `each language picks its own title`() {
        assertEquals("Sousou no Frieren", media.displayTitle(TitleLanguage.ROMAJI))
        assertEquals("Frieren: Beyond Journey's End", media.displayTitle(TitleLanguage.ENGLISH))
        assertEquals("葬送のフリーレン", media.displayTitle(TitleLanguage.NATIVE))
    }

    @Test
    fun `missing or blank titles fall back to the provider title`() {
        assertEquals("Sousou no Frieren", media.copy(titles = emptyMap()).displayTitle(TitleLanguage.ENGLISH))
        assertEquals("Sousou no Frieren", media.copy(titles = mapOf("english" to "  ")).displayTitle(TitleLanguage.ENGLISH))
    }

    @Test
    fun `alternative title skips the shown one and blanks`() {
        assertEquals("葬送のフリーレン", media.alternativeTitle("Sousou no Frieren"))
        assertEquals("Sousou no Frieren", media.alternativeTitle("Frieren: Beyond Journey's End"))
        assertNull(media.copy(titles = mapOf("romaji" to "Sousou no Frieren", "native" to "")).alternativeTitle("Sousou no Frieren"))
    }
}
