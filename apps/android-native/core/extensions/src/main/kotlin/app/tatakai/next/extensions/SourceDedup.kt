// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import java.util.Locale

/**
 * BUG-086: with the same extension installed on the phone and on the linked PC, every source came twice ("Toko · anizone" and
 * "Toko · eidos · anizone", the same SubsPlease torrent twice). A PC source that duplicates a local one is dropped: the local one does
 * not depend on the PC being reachable. Torrents match by info-hash; web sources by extension name (the PC's "Toko · eidos" is "Toko"),
 * server label, quality and audio / subtitle languages (their URLs differ: the PC's go through its media proxy). Pure, JVM-tested.
 */
object SourceDedup {
    fun withoutRemoteDuplicates(local: List<SourceCandidate>, remote: List<SourceCandidate>): List<SourceCandidate> {
        if (local.isEmpty() || remote.isEmpty()) return local + remote
        val keys = local.mapTo(HashSet()) { key(it) }
        return local + remote.filterNot { key(it) in keys }
    }

    internal fun key(c: SourceCandidate): String = when (c.kind) {
        SourceKind.TORRENT -> "t|" + (c.torrent?.infoHash?.lowercase(Locale.ROOT) ?: "${norm(c.extName)}|${norm(c.label)}")
        SourceKind.WEB -> listOf(
            "w",
            norm(baseName(c.extName)),
            norm(c.label),
            c.quality.name,
            c.dubs.map { it.lowercase(Locale.ROOT) }.sorted().joinToString(","),
            c.subs.map { it.lowercase(Locale.ROOT) }.sorted().joinToString(","),
            c.web?.embed == true,
        ).joinToString("|")
    }

    /** "Toko · eidos" (a PC server's name for its copy) -> "Toko". */
    private fun baseName(extName: String): String = extName.substringBefore(" · ")

    private fun norm(s: String): String = s.trim().lowercase(Locale.ROOT)
}
