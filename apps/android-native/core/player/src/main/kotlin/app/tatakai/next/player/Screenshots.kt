// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.File
import java.io.OutputStream
import java.time.LocalDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

// parity wave A (docs/redesign/15 §3.2, checklist player.screenshot / player.android.screenshot). Owner: worker p.
// Frames go to "Pictures/Tatakai" (MediaStore on Android 10+, the app's own Pictures folder before, where no storage permission is
// needed). Media3 frames come from PixelCopy of the video SurfaceView (screen); mpv writes its own PNG (`screenshot-to-file`).

/** Where a screenshot ended up: [displayPath] is what the snackbar shows ("Pictures/Tatakai/x.png"). */
data class SavedScreenshot(val displayPath: String, val uri: String)

object Screenshots {
    const val ALBUM = "Tatakai"

    /** `Tatakai_<title>_E<ep>_<yyyyMMdd-HHmmss>.png`, file-system safe. */
    fun fileName(title: String, episode: Int?, now: LocalDateTime = LocalDateTime.now()): String {
        val clean = title.replace(Regex("[^\\p{L}\\p{N}]+"), "_").trim('_').take(48).ifEmpty { "video" }
        val ep = episode?.let { "_E$it" }.orEmpty()
        return "Tatakai_${clean}${ep}_${now.format(DateTimeFormatter.ofPattern("yyyyMMdd-HHmmss", Locale.ROOT))}.png"
    }

    suspend fun saveBitmap(context: Context, bitmap: Bitmap, name: String): SavedScreenshot? =
        save(context, name) { out -> bitmap.compress(Bitmap.CompressFormat.PNG, 100, out) }

    suspend fun saveFile(context: Context, png: File, name: String): SavedScreenshot? =
        save(context, name) { out -> png.inputStream().use { it.copyTo(out) }; true }

    private suspend fun save(context: Context, name: String, write: (OutputStream) -> Boolean): SavedScreenshot? = withContext(Dispatchers.IO) {
        val app = context.applicationContext
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            val values = ContentValues().apply {
                put(MediaStore.Images.Media.DISPLAY_NAME, name)
                put(MediaStore.Images.Media.MIME_TYPE, "image/png")
                put(MediaStore.Images.Media.RELATIVE_PATH, "${Environment.DIRECTORY_PICTURES}/$ALBUM")
                put(MediaStore.Images.Media.IS_PENDING, 1)
            }
            val resolver = app.contentResolver
            val uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values) ?: return@withContext null
            val ok = runCatching { resolver.openOutputStream(uri)?.use { write(it) } == true }.getOrDefault(false)
            if (!ok) {
                resolver.delete(uri, null, null)
                return@withContext null
            }
            resolver.update(uri, ContentValues().apply { put(MediaStore.Images.Media.IS_PENDING, 0) }, null, null)
            SavedScreenshot("${Environment.DIRECTORY_PICTURES}/$ALBUM/$name", uri.toString())
        } else {
            val dir = File(app.getExternalFilesDir(Environment.DIRECTORY_PICTURES) ?: app.filesDir, ALBUM).apply { mkdirs() }
            val target = File(dir, name)
            val ok = runCatching { target.outputStream().use { write(it) } }.getOrDefault(false)
            if (!ok) {
                target.delete()
                return@withContext null
            }
            SavedScreenshot(target.absolutePath, target.toURI().toString())
        }
    }
}
