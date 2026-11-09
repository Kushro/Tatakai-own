// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import android.content.ContentValues
import android.database.sqlite.SQLiteDatabase
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

/**
 * Downloaded chapter texts (checklist novel.download.chapters: "guarda el texto en SQLite"): a private SQLite file `storeDir/novel.db`
 * opened with the platform `SQLiteDatabase` (no Room, never `TatakaiDatabase`). One row per chapter: id, media id, the
 * `NovelChapterText` JSON and its size.
 */
class NovelTextStore(private val file: File) {
    private val mutex = Mutex()
    private var db: SQLiteDatabase? = null

    private fun open(): SQLiteDatabase {
        db?.takeIf { it.isOpen }?.let { return it }
        file.parentFile?.mkdirs()
        val opened = SQLiteDatabase.openOrCreateDatabase(file, null)
        opened.execSQL(
            "CREATE TABLE IF NOT EXISTS chapter_text (id TEXT PRIMARY KEY NOT NULL, media_id TEXT NOT NULL, body TEXT NOT NULL, " +
                "bytes INTEGER NOT NULL, created_at INTEGER NOT NULL)",
        )
        opened.execSQL("CREATE INDEX IF NOT EXISTS chapter_text_media ON chapter_text(media_id)")
        db = opened
        return opened
    }

    suspend fun put(id: String, mediaId: String, body: String, createdAtMs: Long): Long = io {
        val values = ContentValues().apply {
            put("id", id)
            put("media_id", mediaId)
            put("body", body)
            put("bytes", body.toByteArray(Charsets.UTF_8).size.toLong())
            put("created_at", createdAtMs)
        }
        open().insertWithOnConflict("chapter_text", null, values, SQLiteDatabase.CONFLICT_REPLACE)
        body.toByteArray(Charsets.UTF_8).size.toLong()
    }

    suspend fun get(id: String): String? = io {
        open().rawQuery("SELECT body FROM chapter_text WHERE id = ?", arrayOf(id)).use { c -> if (c.moveToFirst()) c.getString(0) else null }
    }

    suspend fun delete(id: String) = io {
        open().delete("chapter_text", "id = ?", arrayOf(id))
        Unit
    }

    /** Closes and deletes the database file ("Restablecer la app"). */
    suspend fun clear() = io {
        runCatching { db?.close() }
        db = null
        SQLiteDatabase.deleteDatabase(file)
        Unit
    }

    private suspend fun <T> io(block: () -> T): T = withContext(Dispatchers.IO) { mutex.withLock { block() } }

    companion object {
        const val FILE_NAME: String = "novel.db"
    }
}
