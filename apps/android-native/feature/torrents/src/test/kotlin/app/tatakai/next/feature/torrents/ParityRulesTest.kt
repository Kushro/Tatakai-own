// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import app.tatakai.next.feature.torrents.ui.formatMinute
import app.tatakai.next.feature.torrents.ui.parseMinute
import app.tatakai.next.feature.torrents.vm.AddSourceKind
import app.tatakai.next.feature.torrents.vm.classifyAddSource
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** parity wave A (worker q): "Añadir torrent" source detection and the schedule time fields. */
class ParityRulesTest {
    @Test
    fun classifiesSources() {
        assertEquals(AddSourceKind.EMPTY, classifyAddSource("  "))
        assertEquals(AddSourceKind.MAGNET, classifyAddSource("magnet:?xt=urn:btih:${"a".repeat(40)}&dn=x"))
        assertEquals(AddSourceKind.HASH, classifyAddSource("ABCDEF".repeat(6) + "abcd"))
        assertEquals(AddSourceKind.FILE, classifyAddSource("content://com.android.providers.downloads.documents/document/42"))
        assertEquals(AddSourceKind.INVALID, classifyAddSource("magnet:?dn=no-hash"))
        assertEquals(AddSourceKind.INVALID, classifyAddSource("https://nyaa.si/view/1"))
    }

    @Test
    fun parsesAndFormatsMinutes() {
        assertEquals(23 * 60, parseMinute("23:00"))
        assertEquals(7 * 60 + 5, parseMinute("7:05"))
        assertEquals(8 * 60 + 30, parseMinute("0830"))
        assertNull(parseMinute("24:00"))
        assertNull(parseMinute("12:7"))
        assertNull(parseMinute("noon"))
        assertEquals("08:05", formatMinute(485))
        assertEquals("23:59", formatMinute(5000))
    }
}
