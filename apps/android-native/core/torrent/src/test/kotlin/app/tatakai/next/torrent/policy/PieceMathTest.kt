// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class PieceMathTest {

    // file of 1000 bytes starting at byte 250 of a torrent with 100-byte pieces: pieces 2..12
    private val span = FileSpan(offset = 250, size = 1000, pieceLength = 100, numPieces = 20)

    @Test
    fun `piece geometry`() {
        assertEquals(2, span.firstPiece)
        assertEquals(12, span.lastPiece)
        assertEquals(2, span.pieceAt(0))
        assertEquals(3, span.pieceAt(50))
        assertEquals(12, span.pieceAt(5_000))
        assertEquals(50, span.pieceEndInFile(2))
        assertEquals(0, span.pieceStartInFile(2))
        assertEquals(1000, span.pieceEndInFile(12))
    }

    @Test
    fun `complete file is one full range`() {
        assertEquals(listOf(0.0..1.0), downloadedRanges(span) { true })
    }

    @Test
    fun `nothing downloaded is empty`() {
        assertTrue(downloadedRanges(span) { false }.isEmpty())
        assertTrue(downloadedRanges(FileSpan(0, 0, 100, 1)) { true }.isEmpty())
    }

    @Test
    fun `runs of pieces merge into fractions of the file`() {
        val have = setOf(2, 3, 7, 12)
        val ranges = downloadedRanges(span) { it in have }
        assertEquals(3, ranges.size)
        assertEquals(0.0, ranges[0].start)
        assertEquals(0.15, ranges[0].endInclusive, 1e-9) // piece 3 ends at byte 400 of the torrent = 150 of the file
        assertEquals(0.45, ranges[1].start, 1e-9)
        assertEquals(0.55, ranges[1].endInclusive, 1e-9)
        assertEquals(0.95, ranges[2].start, 1e-9)
        assertEquals(1.0, ranges[2].endInclusive, 1e-9)
    }

    @Test
    fun `deadline window covers about 16 MiB`() {
        assertEquals(16, deadlineWindow(1024 * 1024))
        assertEquals(4, deadlineWindow(16 * 1024 * 1024))
        assertEquals(40, deadlineWindow(16 * 1024))
    }
}
