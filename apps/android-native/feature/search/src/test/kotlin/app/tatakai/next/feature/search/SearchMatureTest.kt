// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class SearchMatureTest {
    private fun media(genres: List<String>) = Media(
        ref = MediaRef(anilistId = 1), kind = MediaKind.ANIME, title = "T", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = genres, episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    private val adult = media(listOf("Hentai"))
    private val plain = media(listOf("Action"))

    @Test
    fun `blur mode hides adult covers until revealed`() {
        assertTrue(searchCoverBlurred(MatureContentMode.BLUR, adult, "a:1", emptySet()))
        assertFalse(searchCoverBlurred(MatureContentMode.BLUR, adult, "a:1", setOf("a:1")))
    }

    @Test
    fun `other modes and non adult titles are never blurred`() {
        assertFalse(searchCoverBlurred(MatureContentMode.SHOW, adult, "a:1", emptySet()))
        assertFalse(searchCoverBlurred(MatureContentMode.WARN, adult, "a:1", emptySet()))
        assertFalse(searchCoverBlurred(MatureContentMode.BLUR, plain, "a:1", emptySet()))
    }
}
