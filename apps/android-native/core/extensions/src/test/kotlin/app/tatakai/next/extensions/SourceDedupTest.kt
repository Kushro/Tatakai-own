// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
import kotlin.test.Test
import kotlin.test.assertEquals

/** BUG-086: OG5 listed 94 sources, each local Toko / Latino source again as "… · eidos · …" from the PC. */
class SourceDedupTest {
    private fun web(ext: String, name: String, label: String, url: String, q: Quality = Quality.P1080, dubs: List<String> = emptyList()) =
        SourceCandidate(ext, name, SourceKind.WEB, label, quality = q, dubs = dubs, web = WebInfo(url))

    private fun torrent(ext: String, name: String, hash: String) =
        SourceCandidate(ext, name, SourceKind.TORRENT, "SubsPlease", torrent = TorrentInfo(hash))

    @Test
    fun pcCopiesOfLocalSourcesAreDropped() {
        val local = listOf(
            web("tatakai.extension.toko", "Toko", "anizone", "https://cdn.example/a.m3u8"),
            torrent("tatakai.extension.toko", "Toko", "57e72d46a0c1ba4c4b3dbd0e9af87d45cd0a3fcb"),
        )
        val remote = listOf(
            web("server:pc/toko", "Toko · eidos", "anizone", "http://192.168.1.43:8099/api/server/v1/media/x"),
            torrent("server:pc/toko", "Toko · eidos", "57E72D46A0C1BA4C4B3DBD0E9AF87D45CD0A3FCB"),
            web("server:pc/toko", "Toko · eidos", "anizone", "http://192.168.1.43:8099/api/server/v1/media/y", q = Quality.P720),
            web("server:pc/toko", "Toko · eidos", "anizone", "http://192.168.1.43:8099/api/server/v1/media/z", dubs = listOf("es")),
            web("server:pc/hdb", "HentaiDB · eidos", "x", "http://192.168.1.43:8099/api/server/v1/media/w"),
        )
        val out = SourceDedup.withoutRemoteDuplicates(local, remote)
        assertEquals(local + remote.drop(2), out, "only exact duplicates go; other qualities, dubs and PC-only sources stay")
    }

    @Test
    fun withoutLocalSourcesThePcListIsKept() {
        val remote = listOf(web("server:pc/toko", "Toko · eidos", "anizone", "http://pc/x"))
        assertEquals(remote, SourceDedup.withoutRemoteDuplicates(emptyList(), remote))
    }
}
