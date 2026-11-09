// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import android.content.SharedPreferences
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.io.File
import java.util.Locale

// parity wave A (docs/redesign/15 §3.2 / §3.5, worker p): player settings that have no field in `AppSettings` (closed contract file), kept
// PRIVATE to the player module in its own SharedPreferences ("tatakai_player"), like `SearchHistoryStore` of the search screen. Nothing
// secret lives here (translation keys go to SecretStore).

/**
 * Subtitle look beyond `SubtitleStyle` (checklist player.subs.style.color / .position / .font / .custom-fonts):
 * [textColor] / [outlineColor] are ARGB, [outlineSize] 0..6 (mpv `sub-border-size`, Media3: shadow radius), [positionPct] is mpv
 * `sub-pos` (100 = bottom, 0 = top), [fontFile] the file name of an imported font in [SubtitleFonts.dir] (null = the style font).
 */
data class SubtitleExtras(
    val textColor: Int = WHITE,
    val outlineColor: Int = BLACK,
    val outlineSize: Float = 2.5f,
    val positionPct: Int = 100,
    val fontFile: String? = null,
) {
    fun sanitized(): SubtitleExtras = copy(outlineSize = outlineSize.coerceIn(0f, MAX_OUTLINE), positionPct = positionPct.coerceIn(MIN_POSITION, 100))

    companion object {
        const val WHITE: Int = -0x1 // 0xFFFFFFFF
        const val BLACK: Int = -0x1000000 // 0xFF000000
        const val MAX_OUTLINE = 6f
        const val MIN_POSITION = 0

        /** Swatches of the colour pickers (ARGB): white, yellow, cyan, green, pink, black. */
        val TEXT_COLORS: List<Int> = listOf(WHITE, 0xFFFFEB3B.toInt(), 0xFF80DEEA.toInt(), 0xFFA5D6A7.toInt(), 0xFFF48FB1.toInt(), BLACK)
        val OUTLINE_COLORS: List<Int> = listOf(BLACK, 0xFF424242.toInt(), 0xFF1A237E.toInt(), 0xFF4A148C.toInt(), WHITE)

        /** mpv colour syntax `r/g/b/a` with 0..1 components. */
        fun mpvColor(argb: Int, alpha: Float? = null): String {
            val a = alpha ?: (((argb ushr 24) and 0xFF) / 255f)
            val r = ((argb ushr 16) and 0xFF) / 255f
            val g = ((argb ushr 8) and 0xFF) / 255f
            val b = (argb and 0xFF) / 255f
            return String.format(Locale.ROOT, "%.3f/%.3f/%.3f/%.3f", r, g, b, a.coerceIn(0f, 1f))
        }
    }
}

/** Private, synchronous store of the player's own preferences. One instance per app (Koin `single` in `playerModule`). */
class PlayerPrefs(context: Context) {
    private val prefs: SharedPreferences = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    private val _subtitleExtras = MutableStateFlow(readExtras())
    val subtitleExtras: StateFlow<SubtitleExtras> = _subtitleExtras.asStateFlow()

    fun updateSubtitleExtras(transform: (SubtitleExtras) -> SubtitleExtras) {
        val next = transform(_subtitleExtras.value).sanitized()
        prefs.edit()
            .putInt(K_TEXT_COLOR, next.textColor)
            .putInt(K_OUTLINE_COLOR, next.outlineColor)
            .putFloat(K_OUTLINE_SIZE, next.outlineSize)
            .putInt(K_POSITION, next.positionPct)
            .apply { if (next.fontFile == null) remove(K_FONT) else putString(K_FONT, next.fontFile) }
            .apply()
        _subtitleExtras.value = next
    }

    private fun readExtras(): SubtitleExtras {
        val d = SubtitleExtras()
        return SubtitleExtras(
            textColor = prefs.getInt(K_TEXT_COLOR, d.textColor),
            outlineColor = prefs.getInt(K_OUTLINE_COLOR, d.outlineColor),
            outlineSize = prefs.getFloat(K_OUTLINE_SIZE, d.outlineSize),
            positionPct = prefs.getInt(K_POSITION, d.positionPct),
            fontFile = prefs.getString(K_FONT, null),
        ).sanitized()
    }

    private companion object {
        const val FILE = "tatakai_player"
        const val K_TEXT_COLOR = "sub_text_color"
        const val K_OUTLINE_COLOR = "sub_outline_color"
        const val K_OUTLINE_SIZE = "sub_outline_size"
        const val K_POSITION = "sub_position"
        const val K_FONT = "sub_font_file"
    }
}

/**
 * Fonts the user imports for subtitles (checklist player.subs.custom-fonts): TTF / OTF copied to `filesDir/fonts`. mpv gets the folder as
 * `sub-fonts-dir` (libass also finds the fonts ASS styles ask for there) and, when one is chosen, its family name as `sub-font`; Media3
 * draws with it through Compose `Font(File)`.
 */
class SubtitleFonts(context: Context) {
    val dir: File = File(context.applicationContext.filesDir, "fonts")

    fun list(): List<File> = dir.listFiles { f -> f.isFile && isFontName(f.name) }?.sortedBy { it.name.lowercase() }.orEmpty()

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
