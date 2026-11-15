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
        assertEquals("ASW", missingPickLabel(listOf(toons), asw, loading = false))
    }

    @Test
    fun nothingToSayWhenTheListHasThePickOrAnotherRowIsPicked() {
        val toons = row("ToonsHub", recommended = true)
        assertNull(missingPickLabel(listOf(toons, row("ASW")), asw, loading = false))
        assertNull(missingPickLabel(listOf(toons), toons.key, loading = false))
        assertNull(missingPickLabel(listOf(toons.copy(picked = true)), asw, loading = false))
    }

    @Test
    fun nothingRememberedOrAnEmptyListSaysNothing() {
        val list = listOf(row("ToonsHub", recommended = true))
        assertNull(missingPickLabel(list, null, loading = false))
        assertNull(missingPickLabel(list, "", loading = false))
        assertNull(missingPickLabel(emptyList(), asw, loading = false))
        assertNull(missingPickLabel(list, "malformed", loading = false))
    }

    @Test
    fun aRawInfoHashIsNeverShown() {
        val list = listOf(row("ToonsHub", recommended = true))
        assertNull(missingPickLabel(list, "manual|torrent|${"ab".repeat(20)}", loading = false))
        assertNull(missingPickLabel(list, "manual|torrent|${"AB12".repeat(8)}", loading = false))
    }

    @Test
    fun aLongNameIsShortenedSoTheNoticeStaysOneLine() {
        val list = listOf(row("ToonsHub", recommended = true))
        val name = missingPickLabel(list, "io.github.toko|torrent|[Erai-raws] Magical Explorer - 05 [1080p]", loading = false)!!
        assertTrue(name.length <= MISSING_PICK_MAX_CHARS, name)
        assertTrue(name.endsWith("…"), name)
        // A label with more "|" keeps them (the key splits on the first two only).
        assertEquals("A|B", missingPickLabel(list, "ext|web|A|B", loading = false))
    }

    @Test
    fun nothingIsSaidWhileTheListIsStillArriving() {
        // BUG-142: ME5 on the phone listed 3 → 29 → 32 sources for ~60 s before "Toko · anizone" came in (45).
        val anizone = "tatakai.extension.toko|web|anizone"
        val partial = listOf(row("ToonsHub", recommended = true), row("Erai-raws"), row("Judas"))
        assertNull(missingPickLabel(partial, anizone, loading = true))
        assertNull(missingPickLabel(emptyList(), anizone, loading = true))
        // The list finished without it: now it is said.
        assertEquals("anizone", missingPickLabel(partial, anizone, loading = false))
        // The pick arrived in a later batch: nothing to say once loading ends either.
        assertNull(missingPickLabel(partial + row("anizone").let { r -> r.copy(candidate = r.candidate.copy(extId = "tatakai.extension.toko", kind = SourceKind.WEB)) }, anizone, loading = false))
    }
}
