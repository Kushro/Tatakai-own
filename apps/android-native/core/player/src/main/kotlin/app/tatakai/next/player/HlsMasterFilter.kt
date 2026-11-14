// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import java.net.URI

/**
 * BUG-059: FFmpeg's HLS demuxer (libmpv) opens every variant and every `EXT-X-MEDIA` rendition of a master playlist and reads the first
 * segment of each before the first frame. anizone through the PC's extension server lists 15 audio renditions + 3 video variants; one
 * rendition without segments yet kept mpv at "0:00 / --:--" for > 100 s. Handing mpv a master with only the variant it would play and
 * the renditions in the preferred languages avoids waiting for the others. Pure (unit tested); [MpvPlayerSession] applies it.
 */
object HlsMasterFilter {
    private val ATTR = Regex("""([A-Z0-9-]+)=("[^"]*"|[^,]*)""")
    private val URI_ATTR = Regex("""URI="([^"]*)"""")

    /**
     * The filtered master, or null when [text] is not a master playlist or nothing would be dropped (then the original is played).
     * Keeps one variant (the highest `BANDWIDTH`, what mpv would pick), and per audio / subtitle group the renditions in [audioLang] /
     * [subLang] (ISO 639-1, matched on `LANGUAGE` or `NAME`); a group with no match keeps its `DEFAULT=YES` rendition, else its first one.
     * Renditions without `URI` (muxed into the variant) and closed captions stay. Every URI becomes absolute against [baseUrl].
     */
    fun filter(text: String, baseUrl: String, audioLang: String?, subLang: String?): String? {
        if (!text.trimStart().startsWith("#EXTM3U")) return null
        val lines = text.lines()
        if (lines.none { it.startsWith("#EXT-X-STREAM-INF") }) return null
        val base = runCatching { URI(baseUrl) }.getOrNull() ?: return null

        // Variants: the tag line and its URI line.
        data class Variant(val tag: String, val uri: String, val bandwidth: Long)
        val variants = ArrayList<Variant>()
        val media = ArrayList<Map<String, String>>()
        val mediaLines = ArrayList<String>()
        val others = ArrayList<String>()
        var i = 0
        while (i < lines.size) {
            val line = lines[i].trim()
            when {
                line.startsWith("#EXT-X-STREAM-INF") -> {
                    var j = i + 1
                    while (j < lines.size && (lines[j].isBlank() || lines[j].trim().startsWith("#"))) j++
                    if (j < lines.size) {
                        val attrs = attrs(line.substringAfter(':'))
                        variants += Variant(line, lines[j].trim(), attrs["BANDWIDTH"]?.toLongOrNull() ?: 0)
                    }
                    i = j + 1
                    continue
                }
                line.startsWith("#EXT-X-MEDIA:") -> {
                    media += attrs(line.substringAfter(':'))
                    mediaLines += line
                }
                // I-frame playlists are not played; FFmpeg skips them anyway.
                line.startsWith("#EXT-X-I-FRAME-STREAM-INF") -> Unit
                line.isNotEmpty() -> others += line
            }
            i++
        }
        if (variants.isEmpty()) return null
        val chosen = variants.maxBy { it.bandwidth }
        val chosenAttrs = attrs(chosen.tag.substringAfter(':'))
        val usedGroups = setOfNotNull(chosenAttrs["AUDIO"], chosenAttrs["SUBTITLES"], chosenAttrs["VIDEO"], chosenAttrs["CLOSED-CAPTIONS"])

        val keep = BooleanArray(media.size)
        media.indices.groupBy { media[it]["TYPE"] to media[it]["GROUP-ID"] }.forEach { (key, idx) ->
            val (type, group) = key
            if (group !in usedGroups) return@forEach
            val withUri = idx.filter { media[it]["URI"] != null }
            idx.filter { media[it]["URI"] == null }.forEach { keep[it] = true }
            if (type == "CLOSED-CAPTIONS" || withUri.isEmpty()) return@forEach
            val pref = when (type) {
                "AUDIO" -> audioLang
                "SUBTITLES" -> subLang
                else -> null
            }
            if (type == "VIDEO") {
                withUri.forEach { keep[it] = true }
                return@forEach
            }
            val match = withUri.filter { LanguageCodes.matches(media[it]["LANGUAGE"], media[it]["NAME"], pref) }
            val picked = match.ifEmpty {
                withUri.filter { media[it]["DEFAULT"].equals("YES", ignoreCase = true) }.ifEmpty { listOf(withUri.first()) }.take(1)
            }
            picked.forEach { keep[it] = true }
        }
        val keptMedia = media.indices.count { keep[it] }
        if (variants.size == 1 && keptMedia == media.size) return null

        fun abs(u: String): String = runCatching { base.resolve(u.trim()).toString() }.getOrDefault(u)
        fun absAttrs(line: String): String = URI_ATTR.replace(line) { m -> "URI=\"${abs(m.groupValues[1])}\"" }
        val out = StringBuilder()
        out.append("#EXTM3U\n")
        others.filter { it != "#EXTM3U" }.forEach { out.append(absAttrs(it)).append('\n') }
        media.indices.filter { keep[it] }.forEach { out.append(absAttrs(mediaLines[it])).append('\n') }
        out.append(chosen.tag).append('\n').append(abs(chosen.uri)).append('\n')
        return out.toString()
    }

    private fun attrs(list: String): Map<String, String> =
        ATTR.findAll(list).associate { m -> m.groupValues[1] to m.groupValues[2].removeSurrounding("\"") }
}
