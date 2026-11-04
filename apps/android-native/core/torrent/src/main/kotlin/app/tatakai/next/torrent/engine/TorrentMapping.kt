// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.engine

import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TorrentTracker
import app.tatakai.next.model.torrent.TrackerStatus
import app.tatakai.next.release.parseRelease
import app.tatakai.next.torrent.policy.FileSpan
import app.tatakai.next.torrent.policy.peerFlagLetters
import java.io.File
import org.libtorrent4j.AnnounceEntry
import org.libtorrent4j.PeerInfo
import org.libtorrent4j.Priority
import org.libtorrent4j.TorrentFlags
import org.libtorrent4j.TorrentHandle
import org.libtorrent4j.TorrentInfo
import org.libtorrent4j.TorrentStatus
import org.libtorrent4j.swig.peer_info

// libtorrent4j -> :core:model mapping (desktop tk-app live/torrent.rs mapping, 10 §4.1). Only APIs present in
// .deps-sources/libtorrent4j-2.1.0-39 are used.

/** One non-pad file of a torrent with its geometry. */
internal data class EngineFile(
    val index: Int,
    val path: String,
    val name: String,
    val size: Long,
    val span: FileSpan,
)

internal fun TorrentInfo.engineFiles(): List<EngineFile> {
    val fs = files()
    val count = fs.numFiles()
    val pieceLength = pieceLength()
    val pieces = numPieces()
    return (0 until count).mapNotNull { i ->
        if (fs.padFileAt(i)) return@mapNotNull null
        val size = fs.fileSize(i)
        EngineFile(
            index = i,
            path = fs.filePath(i).replace('\\', '/'),
            name = fs.fileName(i),
            size = size,
            span = FileSpan(offset = fs.fileOffset(i), size = size, pieceLength = pieceLength, numPieces = pieces),
        )
    }
}

/** Absolute location of [file] on disk for [handle]. */
internal fun TorrentHandle.fileOnDisk(file: EngineFile): File = File(savePath(), file.path)

internal fun TorrentStatus.isPausedFlag(): Boolean = flags().and_(TorrentFlags.PAUSED).non_zero()

internal fun TorrentStatus.toModelState(): TorrentState = when {
    errorCode().isError -> TorrentState.ERROR
    isPausedFlag() -> TorrentState.PAUSED
    else -> when (state()) {
        TorrentStatus.State.DOWNLOADING -> TorrentState.DOWNLOADING
        TorrentStatus.State.FINISHED, TorrentStatus.State.SEEDING -> TorrentState.SEEDING
        else -> TorrentState.INITIALIZING
    }
}

/** 0..1 of the selected files. */
internal fun TorrentStatus.wantedProgress(): Double {
    val wanted = totalWanted()
    return if (wanted <= 0) 0.0 else (totalWantedDone().toDouble() / wanted).coerceIn(0.0, 1.0)
}

internal fun Priority.isSelected(): Boolean = this != Priority.IGNORE

/** Episode parsed from a file name (`parseRelease(name).episodes?.first`). */
internal fun episodeOf(name: String): Int? = try {
    parseRelease(name).episodes?.first
} catch (_: Throwable) {
    // the parser is worker a's port; never let it break the Files tab
    null
}

private val FLAG_INTERESTING = peer_info.interesting.to_int()
private val FLAG_CHOKED = peer_info.choked.to_int()
private val FLAG_REMOTE_INTERESTED = peer_info.remote_interested.to_int()
private val FLAG_REMOTE_CHOKED = peer_info.remote_choked.to_int()
private val FLAG_OUTGOING = peer_info.outgoing_connection.to_int()
private val FLAG_UTP = peer_info.utp_socket.to_int()
private val FLAG_RC4 = peer_info.rc4_encrypted.to_int()
private val FLAG_PLAIN_ENCRYPTED = peer_info.plaintext_encrypted.to_int()
private val FLAG_SNUBBED = peer_info.snubbed.to_int()
private val SOURCE_PEX = peer_info.pex.to_int()
private val SOURCE_DHT = peer_info.dht.to_int()

internal fun PeerInfo.toModel(): TorrentPeer {
    val f = flags()
    val src = source().toInt() and 0xff
    fun has(mask: Int) = mask != 0 && (f and mask) != 0
    return TorrentPeer(
        addr = ip(),
        client = client().takeIf { it.isNotBlank() },
        progress = progress().toDouble().coerceIn(0.0, 1.0),
        downBps = downSpeed().toLong().coerceAtLeast(0),
        upBps = upSpeed().toLong().coerceAtLeast(0),
        downloadedBytes = totalDownload(),
        uploadedBytes = totalUpload(),
        country = null, // libtorrent has no GeoIP; desktop shows the code only when known
        flags = peerFlagLetters(
            downloading = has(FLAG_INTERESTING) && !has(FLAG_REMOTE_CHOKED),
            uploading = has(FLAG_REMOTE_INTERESTED) && !has(FLAG_CHOKED),
            incoming = !has(FLAG_OUTGOING),
            utp = has(FLAG_UTP),
            encrypted = has(FLAG_RC4) || has(FLAG_PLAIN_ENCRYPTED),
            fromPex = SOURCE_PEX != 0 && (src and SOURCE_PEX) != 0,
            fromDht = SOURCE_DHT != 0 && (src and SOURCE_DHT) != 0,
            snubbed = has(FLAG_SNUBBED),
        ),
    )
}

internal fun AnnounceEntry.toModel(): TorrentTracker {
    val v1 = endpoints().map { it.infohashV1() }
    val working = v1.any { it.swig().fails.toInt() == 0 && it.swig().message.isNullOrEmpty() && it.swig().start_sent }
    val updating = v1.any { it.updating() }
    val failed = v1.isNotEmpty() && v1.all { it.fails() > 0 }
    val status = when {
        updating -> TrackerStatus.UPDATING
        working -> TrackerStatus.WORKING
        failed -> TrackerStatus.FAILED
        else -> TrackerStatus.UNKNOWN
    }
    fun scrape(pick: (org.libtorrent4j.swig.announce_infohash) -> Int): Int? =
        v1.map { pick(it.swig()) }.filter { it >= 0 }.maxOrNull()
    return TorrentTracker(
        url = url(),
        status = status,
        message = v1.map { it.message() }.firstOrNull { !it.isNullOrBlank() },
        downloaded = scrape { it.scrape_downloaded },
        seeders = scrape { it.scrape_complete },
        leechers = scrape { it.scrape_incomplete },
    )
}
