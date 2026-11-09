// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import app.tatakai.next.model.ChapterPage
import coil3.SingletonImageLoader
import coil3.network.NetworkHeaders
import coil3.network.httpHeaders
import coil3.request.ImageRequest
import coil3.request.SuccessResult
import coil3.request.allowHardware
import coil3.request.transformations
import coil3.size.Size
import coil3.toBitmap
import coil3.transform.Transformation
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File

// Page images of the manga reader: Coil requests with the extension headers (Referer...), border cropping (checklist manga.reader.fit
// "recorte de paneles"), cache warming (prefetch / eager loading) and the page capture (checklist manga.reader.behavior).

internal fun pageRequest(context: Context, page: ChapterPage, cropBorders: Boolean, retry: Int = 0): ImageRequest =
    ImageRequest.Builder(context)
        .data(page.url)
        .apply {
            if (page.headers.isNotEmpty()) {
                httpHeaders(NetworkHeaders.Builder().apply { page.headers.forEach { (k, v) -> set(k, v) } }.build())
            }
            if (cropBorders) transformations(CropBordersTransformation)
            if (retry > 0) memoryCacheKey("${page.url}#retry$retry")
        }
        .build()

/** Loads [pages] into the image cache (prefetch of the next chapter, eager loading, lazy look-ahead). */
internal fun warmPages(context: Context, pages: List<ChapterPage>, cropBorders: Boolean) {
    val loader = SingletonImageLoader.get(context.applicationContext)
    pages.forEach { loader.enqueue(pageRequest(context, it, cropBorders)) }
}

/** Saves the page image to Pictures/Tatakai (MediaStore on API 29+, the app's own Pictures folder before). Returns the display path. */
internal suspend fun capturePage(context: Context, page: ChapterPage, fileStem: String): String? = withContext(Dispatchers.IO) {
    val request = ImageRequest.Builder(context).data(page.url)
        .apply { if (page.headers.isNotEmpty()) httpHeaders(NetworkHeaders.Builder().apply { page.headers.forEach { (k, v) -> set(k, v) } }.build()) }
        .allowHardware(false)
        .build()
    val result = SingletonImageLoader.get(context.applicationContext).execute(request) as? SuccessResult ?: return@withContext null
    val bitmap = result.image.toBitmap()
    val name = fileStem.replace(Regex("[^\\p{L}\\p{N}._-]+"), "_").take(80) + ".png"
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        val values = ContentValues().apply {
            put(MediaStore.Images.Media.DISPLAY_NAME, name)
            put(MediaStore.Images.Media.MIME_TYPE, "image/png")
            put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Tatakai")
        }
        val uri = context.contentResolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values) ?: return@withContext null
        context.contentResolver.openOutputStream(uri)?.use { out -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, out) } ?: return@withContext null
        "Pictures/Tatakai/$name"
    } else {
        val dir = File(context.getExternalFilesDir(Environment.DIRECTORY_PICTURES), "Tatakai").apply { mkdirs() }
        val file = File(dir, name)
        file.outputStream().use { out -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, out) }
        file.absolutePath
    }
}

/** Trims uniform borders (white or black margins of scans) around a page. */
internal object CropBordersTransformation : Transformation() {
    override val cacheKey: String = "tatakai-crop-borders"

    override suspend fun transform(input: Bitmap, size: Size): Bitmap {
        val w = input.width
        val h = input.height
        if (w < 32 || h < 32) return input
        val pixels = IntArray(w * h)
        input.getPixels(pixels, 0, w, 0, 0, w, h)
        val box = BorderCrop.bounds(pixels, w, h) ?: return input
        return Bitmap.createBitmap(input, box[0], box[1], box[2] - box[0], box[3] - box[1])
    }
}

/** Pure border detection over ARGB pixels (JVM-tested). */
internal object BorderCrop {
    private const val TOLERANCE = 24

    /** [left, top, right, bottom) of the content, or null when nothing worth cropping (< 2 % per side or the page is blank). */
    fun bounds(pixels: IntArray, w: Int, h: Int): IntArray? {
        val ref = pixels[0]
        fun near(c: Int): Boolean {
            val dr = ((c shr 16) and 0xff) - ((ref shr 16) and 0xff)
            val dg = ((c shr 8) and 0xff) - ((ref shr 8) and 0xff)
            val db = (c and 0xff) - (ref and 0xff)
            return kotlin.math.abs(dr) <= TOLERANCE && kotlin.math.abs(dg) <= TOLERANCE && kotlin.math.abs(db) <= TOLERANCE
        }
        fun rowBlank(y: Int): Boolean = (0 until w step 2).all { x -> near(pixels[y * w + x]) }
        fun colBlank(x: Int, top: Int, bottom: Int): Boolean = (top until bottom step 2).all { y -> near(pixels[y * w + x]) }
        var top = 0
        while (top < h && rowBlank(top)) top++
        if (top >= h) return null
        var bottom = h
        while (bottom > top && rowBlank(bottom - 1)) bottom--
        var left = 0
        while (left < w && colBlank(left, top, bottom)) left++
        var right = w
        while (right > left && colBlank(right - 1, top, bottom)) right--
        val minX = w / 50
        val minY = h / 50
        if (top < minY && left < minX && h - bottom < minY && w - right < minX) return null
        // Keep a small margin so text is not touching the edge.
        val m = 4
        return intArrayOf((left - m).coerceAtLeast(0), (top - m).coerceAtLeast(0), (right + m).coerceAtMost(w), (bottom + m).coerceAtMost(h))
    }
}
