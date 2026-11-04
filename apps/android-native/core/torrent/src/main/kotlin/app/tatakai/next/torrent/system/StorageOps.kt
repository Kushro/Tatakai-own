// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.system

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.os.storage.StorageManager
import android.provider.MediaStore
import androidx.documentfile.provider.DocumentFile
import app.tatakai.next.torrent.stream.contentTypeFor
import java.io.File
import java.io.IOException

/** Storage helpers of 11 §6: free space, SAF move-on-complete, MediaStore export. All blocking: call on Dispatchers.IO. */
class StorageOps(context: Context) {
    private val app = context.applicationContext

    /** `StorageManager.getAllocatableBytes` for the volume of [dir] (may free cache), else `usableSpace`. */
    fun allocatableBytes(dir: File): Long {
        dir.mkdirs()
        return try {
            val sm = app.getSystemService(StorageManager::class.java) ?: return dir.usableSpace
            sm.getAllocatableBytes(sm.getUuidForPath(dir))
        } catch (_: Exception) {
            dir.usableSpace
        }
    }

    /** Recursive size of [dir]. */
    fun usedBytes(dir: File): Long {
        if (!dir.exists()) return 0
        return dir.walkTopDown().filter { it.isFile }.sumOf { it.length() }
    }

    /**
     * Copies [source] into the SAF tree [treeUri] (`ACTION_OPEN_DOCUMENT_TREE` + persistable permission taken by the settings page)
     * and returns the new document URI. The caller deletes the original afterwards.
     */
    @Throws(IOException::class)
    fun copyToTree(source: File, treeUri: String): String {
        val tree = DocumentFile.fromTreeUri(app, Uri.parse(treeUri)) ?: throw IOException("tree not available")
        if (!tree.canWrite()) throw IOException("tree not writable")
        tree.findFile(source.name)?.delete()
        val target = tree.createFile(contentTypeFor(source.name), source.name) ?: throw IOException("cannot create document")
        app.contentResolver.openOutputStream(target.uri)?.use { out ->
            source.inputStream().use { it.copyTo(out, BUFFER) }
        } ?: throw IOException("cannot open document")
        return target.uri.toString()
    }

    /** Deletes a document created by [copyToTree] (remove with files of a moved torrent). */
    fun deleteDocument(uri: String): Boolean = try {
        DocumentFile.fromSingleUri(app, Uri.parse(uri))?.delete() == true
    } catch (_: Exception) {
        false
    }

    /** "Guardar en Descargas": MediaStore Downloads (Android 10+, no permission), folder `Download/Tatakai`. */
    @Throws(IOException::class)
    fun exportToDownloads(source: File): String {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) throw IOException("unsupported")
        val resolver = app.contentResolver
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, source.name)
            put(MediaStore.MediaColumns.MIME_TYPE, contentTypeFor(source.name))
            put(MediaStore.MediaColumns.RELATIVE_PATH, Environment.DIRECTORY_DOWNLOADS + "/Tatakai")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values) ?: throw IOException("insert failed")
        try {
            resolver.openOutputStream(uri)?.use { out -> source.inputStream().use { it.copyTo(out, BUFFER) } }
                ?: throw IOException("cannot open output")
            values.clear()
            values.put(MediaStore.MediaColumns.IS_PENDING, 0)
            resolver.update(uri, values, null, null)
        } catch (e: IOException) {
            runCatching { resolver.delete(uri, null, null) }
            throw e
        }
        return uri.toString()
    }

    private companion object {
        const val BUFFER = 256 * 1024
    }
}
