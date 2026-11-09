// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.store

import app.tatakai.next.model.torrent.TorrentSessionEntry
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File

/**
 * parity wave A (worker q, checklist torrent.session-history): torrents the session has seen, `filesDir/torrents/history.json`
 * (sibling of `labels.json`), newest first, at most [max] entries. One entry per info-hash: adding it again moves it to the top.
 */
class SessionHistoryStore(private val file: File, private val max: Int = MAX_ENTRIES) {

    @Serializable
    private data class HistoryFile(val version: Int = 1, val entries: List<TorrentSessionEntry> = emptyList())

    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = false; explicitNulls = false }
    private val lock = Any()
    private val state = MutableStateFlow(load())

    val entries: StateFlow<List<TorrentSessionEntry>> = state.asStateFlow()

    /** Records (or refreshes) [entry]; a previous removal stamp is cleared. */
    fun record(entry: TorrentSessionEntry) = mutate { list ->
        val previous = list.firstOrNull { it.id == entry.id }
        val merged = entry.copy(
            downloadedBytes = maxOf(entry.downloadedBytes, previous?.downloadedBytes ?: 0),
            uploadedBytes = maxOf(entry.uploadedBytes, previous?.uploadedBytes ?: 0),
            removedUnix = null,
        )
        (listOf(merged) + list.filter { it.id != entry.id }).take(max)
    }

    /** Marks [id] as removed at [removedUnix] with its final transfer figures (when known). */
    fun markRemoved(id: String, removedUnix: Long, downloaded: Long?, uploaded: Long?) = mutate { list ->
        list.map {
            if (it.id != id) {
                it
            } else {
                it.copy(
                    removedUnix = removedUnix,
                    downloadedBytes = maxOf(it.downloadedBytes, downloaded ?: 0),
                    uploadedBytes = maxOf(it.uploadedBytes, uploaded ?: 0),
                )
            }
        }
    }

    fun clear() = mutate { emptyList() }

    private fun mutate(transform: (List<TorrentSessionEntry>) -> List<TorrentSessionEntry>) {
        synchronized(lock) {
            val next = transform(state.value)
            if (next == state.value) return
            state.value = next
            save(next)
        }
    }

    private fun load(): List<TorrentSessionEntry> = try {
        if (file.isFile) json.decodeFromString(HistoryFile.serializer(), file.readText()).entries.sortedByDescending { it.addedUnix } else emptyList()
    } catch (_: Exception) {
        emptyList()
    }

    private fun save(list: List<TorrentSessionEntry>) {
        try {
            file.parentFile?.mkdirs()
            val tmp = File(file.parentFile, file.name + ".tmp")
            tmp.writeText(json.encodeToString(HistoryFile.serializer(), HistoryFile(entries = list)))
            if (!tmp.renameTo(file)) {
                file.delete()
                tmp.renameTo(file)
            }
        } catch (_: Exception) {
            // best effort
        }
    }

    companion object {
        const val MAX_ENTRIES = 500
    }
}
