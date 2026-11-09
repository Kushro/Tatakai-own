// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

// parity wave A (docs/redesign/15 §3.2). Owner: worker p. Pure, no Android.
// A small SRT / WebVTT / SSA-ASS reader for what the screen draws itself: the secondary subtitle track under Media3, the subtitle delay
// of sidecar tracks under Media3, and the whole-track pre-translation (player.subs.translate.pretranslate). Styling is dropped: only
// timing and plain text (line breaks kept) matter here.

/** One subtitle line set shown from [startMs] (inclusive) to [endMs] (exclusive). */
data class TimedCue(val startMs: Long, val endMs: Long, val text: String)

object SubtitleParser {
    /** Format from a file name / URL / mime: `srt`, `vtt`, `ass` (also for `ssa`), or null when unknown. */
    fun formatOf(nameOrMime: String?): String? {
        val s = nameOrMime?.lowercase()?.substringBefore('?')?.substringBefore('#') ?: return null
        return when {
            s.endsWith(".srt") || s.contains("subrip") || s == "srt" -> "srt"
            s.endsWith(".vtt") || s.contains("vtt") || s == "webvtt" -> "vtt"
            s.endsWith(".ass") || s.endsWith(".ssa") || s.contains("ssa") || s == "ass" -> "ass"
            else -> null
        }
    }

    /** Parses [content]; [format] null = sniff it (`WEBVTT` header, `[Script Info]` / `Dialogue:`, else SRT). Sorted by start. */
    fun parse(content: String, format: String? = null): List<TimedCue> {
        val text = content.removePrefix("﻿").replace("\r\n", "\n").replace('\r', '\n')
        val fmt = format ?: when {
            text.trimStart().startsWith("WEBVTT") -> "vtt"
            text.contains("[Script Info]", ignoreCase = true) || text.contains("\nDialogue:") || text.startsWith("Dialogue:") -> "ass"
            else -> "srt"
        }
        val cues = when (fmt) {
            "ass", "ssa" -> parseAss(text)
            else -> parseBlocks(text)
        }
        return cues.filter { it.text.isNotBlank() && it.endMs > it.startMs }.sortedBy { it.startMs }
    }

    /** Lines visible at [positionMs] (several cues may overlap), in start order. */
    fun activeAt(cues: List<TimedCue>, positionMs: Long): List<String> {
        if (cues.isEmpty()) return emptyList()
        // Cues are sorted by start: binary search the last cue starting at or before the position, then walk back over overlaps.
        var lo = 0
        var hi = cues.size - 1
        var last = -1
        while (lo <= hi) {
            val mid = (lo + hi) ushr 1
            if (cues[mid].startMs <= positionMs) {
                last = mid
                lo = mid + 1
            } else {
                hi = mid - 1
            }
        }
        if (last < 0) return emptyList()
        val out = ArrayList<String>(2)
        var i = last
        var scanned = 0
        while (i >= 0 && scanned < MAX_OVERLAP_SCAN) {
            val c = cues[i]
            if (positionMs < c.endMs) out += c.text
            i--
            scanned++
        }
        return out.asReversed()
    }

    /** Distinct non-blank texts in order of appearance (what pre-translation sends). */
    fun uniqueTexts(cues: List<TimedCue>): List<String> = cues.map { it.text }.filter { it.isNotBlank() }.distinct()

    // ---- SRT / WebVTT ----------------------------------------------------------------------------------------------------------------

    private val ARROW = Regex("""^\s*([0-9:.,]+)\s*-->\s*([0-9:.,]+)""")

    private fun parseBlocks(text: String): List<TimedCue> {
        val out = ArrayList<TimedCue>()
        val lines = text.split('\n')
        var i = 0
        while (i < lines.size) {
            val m = ARROW.find(lines[i])
            if (m == null) {
                i++
                continue
            }
            val start = timestamp(m.groupValues[1])
            val end = timestamp(m.groupValues[2])
            i++
            val body = StringBuilder()
            while (i < lines.size && lines[i].isNotBlank()) {
                if (ARROW.containsMatchIn(lines[i])) break
                if (body.isNotEmpty()) body.append('\n')
                body.append(lines[i])
                i++
            }
            if (start != null && end != null) out += TimedCue(start, end, clean(body.toString()))
        }
        return out
    }

    /** `00:01:02,500`, `01:02.500`, `00:01:02.5` -> ms. */
    internal fun timestamp(s: String): Long? {
        val parts = s.trim().replace(',', '.').split(':')
        if (parts.isEmpty() || parts.size > 3) return null
        val secPart = parts.last()
        val secs = secPart.substringBefore('.').toLongOrNull() ?: return null
        val frac = secPart.substringAfter('.', "").take(3).padEnd(3, '0').toLongOrNull() ?: 0L
        val mins = if (parts.size >= 2) parts[parts.size - 2].toLongOrNull() ?: return null else 0L
        val hours = if (parts.size == 3) parts[0].toLongOrNull() ?: return null else 0L
        return ((hours * 60 + mins) * 60 + secs) * 1000 + frac
    }

    // ---- SSA / ASS ---------------------------------------------------------------------------------------------------------------------

    private fun parseAss(text: String): List<TimedCue> {
        val out = ArrayList<TimedCue>()
        var format: List<String> = DEFAULT_ASS_FORMAT
        var inEvents = false
        for (raw in text.split('\n')) {
            val line = raw.trim()
            if (line.startsWith("[")) {
                inEvents = line.equals("[Events]", ignoreCase = true)
                continue
            }
            if (!inEvents && !line.startsWith("Dialogue:")) continue
            if (line.startsWith("Format:", ignoreCase = true)) {
                format = line.substringAfter(':').split(',').map { it.trim().lowercase() }
                continue
            }
            if (!line.startsWith("Dialogue:", ignoreCase = true)) continue
            val fields = line.substringAfter(':').split(',', limit = format.size)
            if (fields.size < format.size) continue
            val start = assTime(fields[format.indexOf("start").takeIf { it >= 0 } ?: 1])
            val end = assTime(fields[format.indexOf("end").takeIf { it >= 0 } ?: 2])
            val body = fields[format.indexOf("text").takeIf { it >= 0 } ?: (format.size - 1)]
            if (start != null && end != null) out += TimedCue(start, end, clean(body.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ")))
        }
        return out
    }

    /** `0:01:02.50` (centiseconds) -> ms. */
    private fun assTime(s: String): Long? = timestamp(s)

    private val DEFAULT_ASS_FORMAT = listOf("layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text")

    private val ASS_OVERRIDE = Regex("""\{[^}]*\}""")
    private val HTML_TAG = Regex("""</?[a-zA-Z][^>]*>""")

    /** Drops ASS override blocks and HTML-ish tags (`<i>`, `<font>`, `<c.yellow>`), trims every line. */
    internal fun clean(s: String): String =
        s.replace(ASS_OVERRIDE, "").replace(HTML_TAG, "")
            .replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ")
            .split('\n').map { it.trim() }.filter { it.isNotEmpty() }.joinToString("\n")

    private const val MAX_OVERLAP_SCAN = 64
}
