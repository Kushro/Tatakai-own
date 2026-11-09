// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave B (worker r): filters, grouping, ETA and error codes of the Downloads page. */
class DownloadsLogicTest {
    private fun item(id: Int, ep: Int, state: DownloadState, done: Long? = null) = DownloadItem(
        id = "$id?ep=$ep", ref = MediaRef(anilistId = id), title = "Show $id", episode = ep, extId = "e", sourceLabel = "Alpha",
        container = Container.HLS, state = state, totalBytes = 100, createdAtMs = ep.toLong(), completedAtMs = done,
    )

    @Test fun searchMatchesAnyField() {
        assertTrue(DownloadsLogic.matches("", "x"))
        assertTrue(DownloadsLogic.matches("alp", "Show", "Alpha"))
        assertTrue(!DownloadsLogic.matches("zzz", "Show", null))
    }

    @Test fun activeAndGroups() {
        val list = listOf(item(1, 1, DownloadState.COMPLETED, 10), item(1, 2, DownloadState.COMPLETED, 30), item(2, 1, DownloadState.COMPLETED, 20), item(3, 1, DownloadState.DOWNLOADING))
        assertEquals(listOf("3?ep=1"), DownloadsLogic.active(list).map { it.id })
        val groups = DownloadsLogic.groups(list)
        assertEquals(listOf(1, 2), groups.map { it.ref.anilistId })
        assertEquals(listOf(1, 2), groups.first().episodes.map { it.episode })
        assertEquals(200, groups.first().bytes)
    }

    @Test fun etaFromTwoSamples() {
        assertEquals(30, DownloadsLogic.eta(0.0, 0, 0.25, 10_000))
        assertNull(DownloadsLogic.eta(0.5, 0, 0.5, 10_000))
        assertNull(DownloadsLogic.eta(0.0, 0, 1.0, 10_000))
    }

    @Test fun errorCodesAreLocalised() {
        assertEquals("L", codeText("storage_limit", "D", "L", "N", "F"))
        assertEquals("D", codeText("dash_not_supported", "D", "L", "N", "F"))
        assertNull(codeText("other", "D", "L", "N", "F"))
    }
}
