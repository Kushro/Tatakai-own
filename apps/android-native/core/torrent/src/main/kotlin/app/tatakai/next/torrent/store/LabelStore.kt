// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.store

import app.tatakai.next.model.torrent.TorrentLabel
import java.io.File
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/**
 * One torrent known to the app, persisted in `filesDir/torrents/labels.json` (docs/redesign/12 §4 f.1, desktop `labels.json`):
 * the [TorrentLabel] plus `persist`, `seed`, `addedUnix` and what the engine needs to restore it after a restart (the selected
 * files, the magnet for torrents whose resume data is missing) and to show it while the engine is stopped (name, size, progress).
 */
@Serializable
data class StoredTorrent(
    val id: String,
    val name: String = "",
    val magnet: String? = null,
    val label: TorrentLabel? = null,
    val persist: Boolean = false,
    val seed: Boolean = false,
    /** Added through `addDownload` (background download). */
    val background: Boolean = false,
    /** Background download of the whole torrent (`episode == null`). */
    val wholeTorrent: Boolean = false,
    /** Episode wanted by a background download whose metadata had not arrived yet. */
    val pendingEpisode: Int? = null,
    val addedUnix: Long = 0,
    val selected: List<Int> = emptyList(),
    val userPaused: Boolean = false,
    val completed: Boolean = false,
    val files: Int = 0,
    val sizeBytes: Long = 0,
    val progress: Double = 0.0,
    /** `content://` of the SAF folder copy after move-on-complete (11 §6 a); the torrent then leaves the session. */
    val movedUri: String? = null,
)

@Serializable
private data class LabelsFile(val version: Int = 1, val torrents: List<StoredTorrent> = emptyList())

/** Thread-safe store of [StoredTorrent]s backed by an atomically rewritten JSON file. */
class LabelStore(private val file: File) {

    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = false
        explicitNulls = false
    }

    private val lock = Any()
    private val state = MutableStateFlow(load())

    /** id -> torrent. */
    val torrents: StateFlow<Map<String, StoredTorrent>> = state.asStateFlow()

    operator fun get(id: String): StoredTorrent? = state.value[id]

    fun upsert(torrent: StoredTorrent) = mutate { it + (torrent.id to torrent) }

    /** Applies [transform] to an existing entry; no-op when [id] is unknown. Returns the new value. */
    fun update(id: String, transform: (StoredTorrent) -> StoredTorrent): StoredTorrent? {
        var result: StoredTorrent? = null
        mutate { map ->
            val current = map[id] ?: return@mutate map
            val next = transform(current)
            result = next
            if (next == current) map else map + (id to next)
        }
        return result
    }

    fun remove(id: String) = mutate { it - id }

    private fun mutate(transform: (Map<String, StoredTorrent>) -> Map<String, StoredTorrent>) {
        synchronized(lock) {
            val before = state.value
            val after = transform(before)
            if (after === before) return
            state.value = after
            save(after)
        }
    }

    private fun load(): Map<String, StoredTorrent> = try {
        if (!file.isFile) {
            emptyMap()
        } else {
            json.decodeFromString(LabelsFile.serializer(), file.readText()).torrents.associateBy { it.id }
        }
    } catch (_: Exception) {
        // A corrupt file must not break the app: start empty (the resume files stay on disk).
        emptyMap()
    }

    private fun save(map: Map<String, StoredTorrent>) {
        try {
            file.parentFile?.mkdirs()
            val tmp = File(file.parentFile, file.name + ".tmp")
            tmp.writeText(json.encodeToString(LabelsFile.serializer(), LabelsFile(torrents = map.values.sortedBy { it.addedUnix })))
            if (!tmp.renameTo(file)) {
                file.delete()
                tmp.renameTo(file)
            }
        } catch (_: Exception) {
            // best effort; the in-memory state stays authoritative for this process
        }
    }
}
