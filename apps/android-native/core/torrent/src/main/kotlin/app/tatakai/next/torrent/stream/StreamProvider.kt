// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.stream

import java.io.Closeable
import java.io.IOException

/** Size and name of a streamable file (for HEAD and the response headers). */
internal data class StreamInfo(val name: String, val size: Long)

/** One open read stream of a torrent file. Reads block until the piece holding [read]'s position is downloaded. */
internal interface StreamHandle : Closeable {
    val info: StreamInfo

    /**
     * Reads up to [length] bytes at [position] of the file into [buffer]. Blocks until data is available; returns the number of bytes
     * read (> 0) or -1 at end of file. Throws [IOException] on timeout, when the torrent disappears or after [close].
     */
    @Throws(IOException::class)
    fun read(position: Long, buffer: ByteArray, offset: Int, length: Int): Int
}

/** What the loopback server serves: the torrent engine in production, an in-memory fake in tests. */
internal interface StreamProvider {
    /** null = unknown torrent / file (404). Must not start downloading. */
    fun info(infoHash: String, fileIndex: Int): StreamInfo?

    /** null = unknown torrent / file (404). Opening counts as an active stream until [StreamHandle.close]. */
    fun open(infoHash: String, fileIndex: Int): StreamHandle?
}
