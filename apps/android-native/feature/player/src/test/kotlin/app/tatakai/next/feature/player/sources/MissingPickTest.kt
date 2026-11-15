// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player.sources

import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.TorrentInfo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

// Port of the desktop test `a_remembered_pick_missing_from_the_list_is_named` (tk-ui/src/components/sources.rs, BUG-140).
class MissingPickTest {
    private fun row(label: String, recommended: Boolean = false, picked: Boolean = false) = RankedSource(
        candidate = SourceCandidate(
            extId = "io.github.toko",
            extName = "Toko",
            kind = SourceKind.TORRENT,
            label = label,
            quality = Quality.P1080,
            reliability = 0.8f,
            torrent = TorrentInfo("b".repeat(40)),
        ),
        score = 0f,
        recommended = recommended,
        picked = picked,
    )

    private val asw = "io.github.toko|torrent|ASW"

    @Test
    fun aRememberedPickMissingFromTheListIsNamed() {
        val toons = row("ToonsHub", recommended = true)
        assertEquals("ASW", missingPickLabel(listOf(toons), asw))
    }

    @Test
    fun nothingToSayWhenTheListHasThePickOrAnotherRowIsPicked() {
        val toons = row("ToonsHub", recommended = true)
        assertNull(missingPickLabel(listOf(toons, row("ASW")), asw))
        assertNull(missingPickLabel(listOf(toons), toons.key))
        assertNull(missingPickLabel(listOf(toons.copy(picked = true)), asw))
    }

    @Test
    fun nothingRememberedOrAnEmptyListSaysNothing() {
        val list = listOf(row("ToonsHub", recommended = true))
        assertNull(missingPickLabel(list, null))
        assertNull(missingPickLabel(list, ""))
        assertNull(missingPickLabel(emptyList(), asw))
        assertNull(missingPickLabel(list, "malformed"))
    }

    @Test
    fun aRawInfoHashIsNeverShown() {
        val list = listOf(row("ToonsHub", recommended = true))
        assertNull(missingPickLabel(list, "manual|torrent|${"ab".repeat(20)}"))
        assertNull(missingPickLabel(list, "manual|torrent|${"AB12".repeat(8)}"))
    }

    @Test
    fun aLongNameIsShortenedSoTheNoticeStaysOneLine() {
        val list = listOf(row("ToonsHub", recommended = true))
        val name = missingPickLabel(list, "io.github.toko|torrent|[Erai-raws] Magical Explorer - 05 [1080p]")!!
        assertTrue(name.length <= MISSING_PICK_MAX_CHARS, name)
        assertTrue(name.endsWith("…"), name)
        // A label with more "|" keeps them (the key splits on the first two only).
        assertEquals("A|B", missingPickLabel(list, "ext|web|A|B"))
    }
}
