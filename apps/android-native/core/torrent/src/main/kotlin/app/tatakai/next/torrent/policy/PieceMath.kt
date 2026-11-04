// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

// Piece arithmetic of one file inside a torrent (seek-bar marks, read deadlines). Pure, JVM-tested.

/** A file of a torrent: [offset] inside the torrent's concatenated data, [size], and the torrent's piece geometry. */
data class FileSpan(val offset: Long, val size: Long, val pieceLength: Int, val numPieces: Int) {
    init {
        require(pieceLength > 0) { "pieceLength must be > 0" }
    }

    /** Piece holding byte [position] of the file (clamped to the file). */
    fun pieceAt(position: Long): Int {
        val p = position.coerceIn(0, (size - 1).coerceAtLeast(0))
        return ((offset + p) / pieceLength).toInt().coerceIn(0, (numPieces - 1).coerceAtLeast(0))
    }

    val firstPiece: Int get() = pieceAt(0)

    val lastPiece: Int get() = pieceAt(size - 1)

    /** End (exclusive, file coordinates) of the bytes of [piece] that belong to this file. */
    fun pieceEndInFile(piece: Int): Long = ((piece + 1).toLong() * pieceLength - offset).coerceAtMost(size)

    /** Start (file coordinates) of the bytes of [piece] that belong to this file. */
    fun pieceStartInFile(piece: Int): Long = (piece.toLong() * pieceLength - offset).coerceAtLeast(0)
}

/**
 * Downloaded ranges of the file as fractions `0..1` (merged runs of consecutive pieces), what `TorrentRepository.seekMarks`
 * returns. An empty file or no piece yields an empty list; a complete file yields `[0.0..1.0]`.
 */
fun downloadedRanges(span: FileSpan, have: (Int) -> Boolean): List<ClosedFloatingPointRange<Double>> {
    if (span.size <= 0) return emptyList()
    val ranges = ArrayList<ClosedFloatingPointRange<Double>>()
    var runStart = -1L
    var runEnd = -1L
    for (piece in span.firstPiece..span.lastPiece) {
        if (have(piece)) {
            val start = span.pieceStartInFile(piece)
            val end = span.pieceEndInFile(piece)
            if (runStart < 0) {
                runStart = start
            }
            runEnd = end
        } else if (runStart >= 0) {
            ranges += (runStart.toDouble() / span.size)..(runEnd.toDouble() / span.size)
            runStart = -1
        }
    }
    if (runStart >= 0) ranges += (runStart.toDouble() / span.size)..(runEnd.toDouble() / span.size)
    return ranges
}

/** How many pieces ahead of the read position get deadlines: about 16 MiB of video, between 4 and 40 pieces. */
fun deadlineWindow(pieceLength: Int): Int = (WINDOW_BYTES / pieceLength.coerceAtLeast(1)).toInt().coerceIn(4, 40)

private const val WINDOW_BYTES = 16L * 1024 * 1024
