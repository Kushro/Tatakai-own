// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.Media
import app.tatakai.next.model.StreamSource
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import java.io.File

internal val DownloadJson = Json { ignoreUnknownKeys = true; encodeDefaults = true }

/**
 * One web download as persisted by :core:download: the public [item] plus what is needed to resume, re-export or retry it after a
 * restart ([stream] with the extension headers, the HLS media playlist actually downloaded, the [media] for a retry that re-resolves).
 */
@Serializable
internal data class StoredDownload(
    val item: DownloadItem,
    val stream: StreamSource,
    /** URL handed to the Media3 `DownloadManager` (the chosen HLS media playlist, or the MP4 url). */
    val requestUrl: String,
    val media: Media? = null,
)

/** Private JSON stores under `filesDir/downloads/` (atomic write: `.tmp` + rename). Missing or corrupt = empty. */
internal class DownloadStore(private val dir: File) {
    private fun <T> read(name: String, serializer: KSerializer<List<T>>): List<T> {
        val f = File(dir, name)
        return try {
            if (f.isFile) DownloadJson.decodeFromString(serializer, f.readText()) else emptyList()
        } catch (_: Exception) {
            emptyList()
        }
    }

    @Synchronized
    private fun <T> write(name: String, serializer: KSerializer<List<T>>, value: List<T>) {
        dir.mkdirs()
        val f = File(dir, name)
        val tmp = File(dir, "$name.tmp")
        tmp.writeText(DownloadJson.encodeToString(serializer, value))
        if (!tmp.renameTo(f)) {
            f.delete()
            tmp.renameTo(f)
        }
    }

    fun downloads(): List<StoredDownload> = read(DOWNLOADS, ListSerializer(StoredDownload.serializer()))
    fun saveDownloads(v: List<StoredDownload>) = write(DOWNLOADS, ListSerializer(StoredDownload.serializer()), v)
    fun rules(): List<AutoDownloadRule> = read(RULES, ListSerializer(AutoDownloadRule.serializer()))
    fun saveRules(v: List<AutoDownloadRule>) = write(RULES, ListSerializer(AutoDownloadRule.serializer()), v)
    fun library(): List<LocalLibraryItem> = read(LIBRARY, ListSerializer(LocalLibraryItem.serializer()))
    fun saveLibrary(v: List<LocalLibraryItem>) = write(LIBRARY, ListSerializer(LocalLibraryItem.serializer()), v)

    /** Cleared history entries (ids hidden from Downloads › Historial; the files of completed ones stay). */
    fun hiddenHistory(): List<String> = read(HIDDEN, ListSerializer(String.serializer()))
    fun saveHiddenHistory(v: List<String>) = write(HIDDEN, ListSerializer(String.serializer()), v)

    companion object {
        const val DOWNLOADS = "downloads.json"
        const val RULES = "rules.json"
        const val LIBRARY = "library.json"
        const val HIDDEN = "history-hidden.json"
    }
}
