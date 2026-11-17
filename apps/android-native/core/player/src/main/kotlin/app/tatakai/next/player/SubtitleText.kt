// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

// 0.4.0 (sub-translation worker): plain text of subtitle events, the same whatever produced them. The screen gets its live lines
// from mpv `sub-text` (one string with every active event, override tags stripped by mpv) or from Media3 cues (one per event), while
// look-ahead / pre-translation reads the events themselves (sidecar file, or the embedded track via [EmbeddedSubtitleExtractor]).
// All of them go through [SubtitleText.normalize] so the translation map has one key per event, and [SubtitleText.lookup] rebuilds a
// multi-event live string from the per-event translations. Pure, no Android.

object SubtitleText {
    private val DRAWING_TAG = Regex("""\\p(\d+)""")
    private val KARAOKE_TAG = Regex("""\\(?:k|K|kf|ko)\d""")
    private val SPACES = Regex("""[ \t\u00A0\u2007\u202F]+""")

    /** A line made only of ASS vector commands (`m 0 0 l 100 0 100 50`): what mpv / Media3 leave of a `{\p1}` drawing. */
    private val DRAWING_LINE = Regex("""^\s*[mn]\s+-?\d+(?:\.\d+)?\s+-?\d+(?:\.\d+)?(?:\s+(?:[mnlbspc]|-?\d+(?:\.\d+)?))*\s*$""")

    /** Trimmed lines, no blank ones, NBSP / runs of spaces folded to one space. The key of every translation. */
    fun normalize(text: String): String =
        text.replace("\r\n", "\n").replace('\r', '\n').split('\n')
            .map { it.replace(SPACES, " ").trim() }
            .filter { it.isNotEmpty() && !isDrawingLike(it) }
            .joinToString("\n")

    /** Leftover vector drawing commands (never translated, never drawn by the screen). */
    fun isDrawingLike(line: String): Boolean = line.length >= 7 && DRAWING_LINE.matches(line)

    /** What one ASS / SSA event's `Text` field turned into. */
    data class AssText(val plain: String, val karaoke: Boolean, val drawingOnly: Boolean)

    /**
     * ASS `Text` field -> plain text: override blocks dropped, the text of `{\p1}...{\p0}` drawings dropped, `\N` / `\n` line breaks,
     * `\h` hard space. [AssText.karaoke] = it has `\k` timing tags (song lyrics: shown as is, not translated).
     */
    fun fromAss(field: String): AssText {
        val out = StringBuilder(field.length)
        var drawing = false
        var karaoke = false
        var hadDrawing = false
        var i = 0
        while (i < field.length) {
            val c = field[i]
            if (c == '{') {
                val close = field.indexOf('}', i + 1)
                if (close < 0) {
                    // Unclosed block: libass shows it literally; keep it as text.
                    if (!drawing) out.append(field, i, field.length)
                    break
                }
                val block = field.substring(i + 1, close)
                DRAWING_TAG.findAll(block).lastOrNull()?.let { m ->
                    drawing = (m.groupValues[1].toIntOrNull() ?: 0) > 0
                    if (drawing) hadDrawing = true
                }
                if (KARAOKE_TAG.containsMatchIn(block)) karaoke = true
                i = close + 1
                continue
            }
            if (c == '\\' && i + 1 < field.length) {
                when (field[i + 1]) {
                    'N', 'n' -> {
                        if (!drawing) out.append('\n')
                        i += 2
                        continue
                    }
                    'h' -> {
                        if (!drawing) out.append(' ')
                        i += 2
                        continue
                    }
                }
            }
            if (!drawing) out.append(c)
            i++
        }
        val plain = normalize(out.toString())
        return AssText(plain, karaoke, drawingOnly = hadDrawing && plain.isEmpty())
    }

    /**
     * Translation of a live string: the exact key, else the string rebuilt from consecutive lines that are known keys (mpv joins
     * simultaneous events with '\n'; each event may itself span lines). Lines without a translation stay as they are; null when no
     * part is translated.
     */
    fun lookup(translated: Map<String, String>, text: String): String? {
        if (translated.isEmpty()) return null
        val key = normalize(text)
        translated[key]?.let { return it }
        val lines = key.split('\n')
        if (lines.size < 2) return null
        val out = ArrayList<String>(lines.size)
        var any = false
        var i = 0
        while (i < lines.size) {
            var matched = false
            // Longest run first: a two-line event must win over its first line alone.
            for (j in lines.size downTo i + 1) {
                val run = lines.subList(i, j).joinToString("\n")
                val t = translated[run] ?: continue
                out += t
                i = j
                matched = true
                any = true
                break
            }
            if (!matched) {
                out += lines[i]
                i++
            }
        }
        return if (any) out.joinToString("\n") else null
    }

    /** Lines of [text] (normalized) that [lookup] cannot cover: what still needs a request. Empty when fully known. */
    fun missingParts(translated: Map<String, String>, text: String): List<String> {
        val key = normalize(text)
        if (key.isEmpty() || key in translated) return emptyList()
        val lines = key.split('\n')
        val out = ArrayList<String>()
        var i = 0
        var pending = ArrayList<String>()
        while (i < lines.size) {
            var matched = false
            for (j in lines.size downTo i + 1) {
                if (lines.subList(i, j).joinToString("\n") in translated) {
                    if (pending.isNotEmpty()) {
                        out += pending.joinToString("\n")
                        pending = ArrayList()
                    }
                    i = j
                    matched = true
                    break
                }
            }
            if (!matched) {
                pending += lines[i]
                i++
            }
        }
        if (pending.isNotEmpty()) out += pending.joinToString("\n")
        // Nothing of it known: ask for the whole string as one unit (keeps a multi-line event together).
        return if (out.size == 1 && out[0] == key) listOf(key) else out
    }
}
