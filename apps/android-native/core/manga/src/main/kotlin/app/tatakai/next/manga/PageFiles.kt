// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ChapterPage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException

/** Downloads chapter page images to disk (checklist manga.download.chapters) and lists them back as `file://` pages (offline read). */
object PageFiles {
    private val EXTENSIONS = setOf("jpg", "jpeg", "png", "webp", "gif", "avif")

    /** `001.jpg` style name from the index and the URL / content type. */
    fun fileName(index: Int, url: String, contentType: String?): String {
        val fromType = when {
            contentType == null -> null
            contentType.contains("png") -> "png"
            contentType.contains("webp") -> "webp"
            contentType.contains("gif") -> "gif"
            contentType.contains("avif") -> "avif"
            contentType.contains("jpeg") || contentType.contains("jpg") -> "jpg"
            else -> null
        }
        val fromUrl = url.substringBefore('?').substringAfterLast('/').substringAfterLast('.', "").lowercase().takeIf { it in EXTENSIONS }
        return "%04d.%s".format(index, fromType ?: fromUrl ?: "jpg")
    }

    /** Downloads one page into [dir]; returns its size in bytes. */
    suspend fun download(client: OkHttpClient, page: ChapterPage, dir: File): Long = withContext(Dispatchers.IO) {
        dir.mkdirs()
        if (page.url.startsWith("file:")) {
            val src = File(java.net.URI(page.url))
            val target = File(dir, fileName(page.index, page.url, null))
            src.copyTo(target, overwrite = true)
            return@withContext target.length()
        }
        val request = Request.Builder().url(page.url).apply { page.headers.forEach { (k, v) -> header(k, v) } }.get().build()
        try {
            client.newCall(request).execute().use { response ->
                if (!response.isSuccessful) throw AppException(AppError.Http(response.code))
                val body = response.body
                val target = File(dir, fileName(page.index, page.url, body.contentType()?.toString()))
                val tmp = File(dir, target.name + ".part")
                tmp.outputStream().use { out -> body.byteStream().use { it.copyTo(out) } }
                if (!tmp.renameTo(target)) {
                    target.delete()
                    tmp.renameTo(target)
                }
                target.length()
            }
        } catch (e: IOException) {
            throw AppException(AppError.Offline, e)
        }
    }

    /** The downloaded pages of [dir], in index order. */
    fun list(dir: File): List<ChapterPage> =
        (dir.listFiles()?.filter { it.isFile && !it.name.endsWith(".part") && it.extension.lowercase() in EXTENSIONS } ?: emptyList())
            .sortedBy { it.name }
            .mapIndexed { i, f -> ChapterPage(i, "file://" + f.absolutePath) }
}
