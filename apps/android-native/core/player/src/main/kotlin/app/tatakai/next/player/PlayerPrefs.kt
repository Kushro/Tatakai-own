// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import android.content.SharedPreferences
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SubtitleStyle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import java.io.File

// parity wave A (docs/redesign/15 §3.2 / §3.5, worker p): player settings that had no field in `AppSettings` were kept PRIVATE to the
// player module in its own SharedPreferences ("tatakai_player"). 0.4.1: the only ones (the subtitle colours, outline size, position and
// imported font, 0.4.0's `SubtitleExtras`) moved into `AppSettings.subtitleStyle` (DataStore); this class now only migrates them once.

/**
 * 0.4.0's private subtitle extras as stored in "tatakai_player" (`sub_text_color`, `sub_outline_color`, `sub_outline_size`,
 * `sub_position`, `sub_font_file`). Pure: built from `SharedPreferences.getAll()` so the migration is unit-tested on the JVM.
 */
data class LegacySubtitleExtras(
    val textColor: Int? = null,
    val outlineColor: Int? = null,
    val outlineSize: Float? = null,
    val positionPct: Int? = null,
    val fontFile: String? = null,
) {
    fun applyTo(style: SubtitleStyle): SubtitleStyle = style.withLegacyExtras(textColor, outlineColor, outlineSize, positionPct, fontFile)

    companion object {
        const val K_TEXT_COLOR = "sub_text_color"
        const val K_OUTLINE_COLOR = "sub_outline_color"
        const val K_OUTLINE_SIZE = "sub_outline_size"
        const val K_POSITION = "sub_position"
        const val K_FONT = "sub_font_file"
        val KEYS: List<String> = listOf(K_TEXT_COLOR, K_OUTLINE_COLOR, K_OUTLINE_SIZE, K_POSITION, K_FONT)

        /** The stored extras, or null when the user never changed them (0.4.0 wrote every key on the first change). */
        fun from(values: Map<String, *>): LegacySubtitleExtras? {
            if (KEYS.none { it in values }) return null
            return LegacySubtitleExtras(
                textColor = values[K_TEXT_COLOR] as? Int,
                outlineColor = values[K_OUTLINE_COLOR] as? Int,
                outlineSize = (values[K_OUTLINE_SIZE] as? Float)?.takeIf { it.isFinite() },
                positionPct = values[K_POSITION] as? Int,
                fontFile = (values[K_FONT] as? String)?.takeIf { it.isNotBlank() },
            )
        }
    }
}

/**
 * The player's private SharedPreferences. One instance per app (Koin `single(createdAtStart = true)` in `playerModule`): at start it
 * folds 0.4.0's [LegacySubtitleExtras] into `AppSettings.subtitleStyle` (DataStore) and deletes them, so nobody loses their subtitle look.
 */
class PlayerPrefs(
    context: Context,
    settings: SettingsRepository? = null,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
) {
    private val appContext = context.applicationContext

    init {
        if (settings != null) {
            scope.launch {
                val prefs: SharedPreferences = appContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)
                val legacy = LegacySubtitleExtras.from(prefs.all) ?: return@launch
                // Deleted only after the DataStore write succeeded: a failure retries on the next start.
                runCatching { settings.update { it.copy(subtitleStyle = legacy.applyTo(it.subtitleStyle)) } }
                    .onSuccess { prefs.edit().apply { LegacySubtitleExtras.KEYS.forEach { k -> remove(k) } }.apply() }
            }
        }
    }

    private companion object {
        const val FILE = "tatakai_player"
    }
}

/**
 * Fonts the user imports for subtitles (checklist player.subs.custom-fonts): TTF / OTF copied to `filesDir/fonts`. mpv gets the folder as
 * `sub-fonts-dir` (libass also finds the fonts ASS styles ask for there) and, when one is chosen, its family name as `sub-font`; Media3
 * draws with it through Compose `Font(File)`.
 *
 * 0.4.1: the same folder also holds static copies of the app's own fonts for libass (files named [BUNDLED_PREFIX]..., installed by
 * :core:player-mpv); they are not user fonts, so [list] hides them and [import] never writes over them.
 */
class SubtitleFonts(context: Context) {
    val dir: File = File(context.applicationContext.filesDir, "fonts")

    fun list(): List<File> =
        dir.listFiles { f -> f.isFile && isFontName(f.name) && !f.name.startsWith(BUNDLED_PREFIX) }?.sortedBy { it.name.lowercase() }.orEmpty()

    fun file(name: String?): File? = name?.let { File(dir, it) }?.takeIf { it.isFile }

    /** Family name of [name] (from the font's `name` table), or its file name without extension. */
    fun familyName(name: String): String? {
        val f = file(name) ?: return null
        val bytes = runCatching { f.readBytes() }.getOrNull() ?: return null
        return FontNames.family(bytes) ?: f.nameWithoutExtension
    }

    /** Copies [input] as [displayName] (sanitised, `.ttf` / `.otf` only). Returns the stored file name or null when refused. */
    fun import(displayName: String, input: java.io.InputStream): String? {
        val safe = displayName.substringAfterLast('/').replace(Regex("[^A-Za-z0-9._ -]"), "_").take(80)
            .let { if (it.startsWith(BUNDLED_PREFIX)) "user_$it" else it }
        if (!isFontName(safe)) return null
        dir.mkdirs()
        val target = File(dir, safe)
        input.use { src -> target.outputStream().use { src.copyTo(it) } }
        if (target.length() == 0L || target.length() > MAX_FONT_BYTES) {
            target.delete()
            return null
        }
        return safe
    }

    fun delete(name: String): Boolean = file(name)?.delete() == true

    companion object {
        const val MAX_FONT_BYTES = 30L * 1024 * 1024

        /** File-name prefix of the app's own fonts installed for libass (`tatakai_outfit_medium.ttf`...). */
        const val BUNDLED_PREFIX = "tatakai_"

        fun isFontName(name: String): Boolean = name.lowercase().let { it.endsWith(".ttf") || it.endsWith(".otf") || it.endsWith(".ttc") }
    }
}

/** Reads the family name (name id 1, or 16 when present) of a TrueType / OpenType font. Pure, no Android. */
object FontNames {
    fun family(bytes: ByteArray): String? = runCatching { parse(bytes) }.getOrNull()

    private fun parse(b: ByteArray): String? {
        var base = 0
        // TrueType collection: use the first font.
        if (b.size >= 16 && String(b, 0, 4, Charsets.US_ASCII) == "ttcf") base = u32(b, 12).toInt()
        val numTables = u16(b, base + 4)
        var nameOffset = -1
        for (i in 0 until numTables) {
            val rec = base + 12 + i * 16
            if (String(b, rec, 4, Charsets.US_ASCII) == "name") {
                nameOffset = u32(b, rec + 8).toInt()
                break
            }
        }
        if (nameOffset < 0) return null
        val count = u16(b, nameOffset + 2)
        val strings = nameOffset + u16(b, nameOffset + 4)
        var best: String? = null
        var bestRank = Int.MAX_VALUE
        for (i in 0 until count) {
            val r = nameOffset + 6 + i * 12
            val platform = u16(b, r)
            val nameId = u16(b, r + 6)
            if (nameId != 1 && nameId != 16) continue
            val len = u16(b, r + 8)
            val off = strings + u16(b, r + 10)
            if (off + len > b.size) continue
            val text = when (platform) {
                0, 3 -> String(b, off, len, Charsets.UTF_16BE)
                1 -> String(b, off, len, Charsets.ISO_8859_1)
                else -> continue
            }.trim()
            if (text.isEmpty()) continue
            // Typographic family (16) beats the legacy family (1); Windows / Unicode beat Mac.
            val rank = (if (nameId == 16) 0 else 2) + (if (platform == 1) 1 else 0)
            if (rank < bestRank) {
                best = text
                bestRank = rank
            }
        }
        return best
    }

    private fun u16(b: ByteArray, at: Int): Int = ((b[at].toInt() and 0xFF) shl 8) or (b[at + 1].toInt() and 0xFF)

    private fun u32(b: ByteArray, at: Int): Long =
        ((b[at].toLong() and 0xFF) shl 24) or ((b[at + 1].toLong() and 0xFF) shl 16) or ((b[at + 2].toLong() and 0xFF) shl 8) or (b[at + 3].toLong() and 0xFF)
}
