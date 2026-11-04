// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import java.util.Locale

// Private helpers of Sources.kt, ported from apps/desktop-native/crates/tk-core/src/sources.rs
// (Quality::parse, container_label, parse_container, container_from_url, lang_codes). docs/redesign/12 §3.1.

/** Port of `Quality::parse`: explicit heights first ("1080p", "1920x1080", "720"), then the usual marketing names. */
internal fun parseQuality(raw: String?): Quality {
    if (raw == null) return Quality.UNKNOWN
    val s = raw.lowercase(Locale.ROOT)
    for (run in s.split(Regex("[^0-9]+"))) {
        when (run) {
            "2160", "4320" -> return Quality.P2160
            "1440" -> return Quality.P1440
            "1080" -> return Quality.P1080
            "720" -> return Quality.P720
            "480" -> return Quality.P480
        }
    }
    if (s.contains("full hd")) return Quality.P1080
    for (word in s.split(Regex("[^0-9a-z]+"))) {
        when (word) {
            "4k", "uhd", "8k" -> return Quality.P2160
            "2k", "qhd" -> return Quality.P1440
            "fhd", "fullhd" -> return Quality.P1080
            "hd" -> return Quality.P720
            "sd" -> return Quality.P480
        }
    }
    return Quality.UNKNOWN
}

/**
 * `HLS` / `MP4` / `DASH`: the display string a web candidate carries in `container`. [Container.PROGRESSIVE] (Android only, torrent
 * loopback) shows as `FILE` and parses back through [parseContainer].
 */
internal fun containerLabel(c: Container): String = when (c) {
    Container.HLS -> "HLS"
    Container.MP4 -> "MP4"
    Container.DASH -> "DASH"
    Container.PROGRESSIVE -> "FILE"
}

internal fun parseContainer(s: String): Container? = when (s.trim().lowercase(Locale.ROOT)) {
    "hls", "m3u8" -> Container.HLS
    "mp4" -> Container.MP4
    "dash", "mpd" -> Container.DASH
    "file", "progressive" -> Container.PROGRESSIVE
    else -> null
}

internal fun containerFromUrl(url: String): Container? {
    val cut = url.indexOfAny(charArrayOf('?', '#'))
    val path = (if (cut >= 0) url.substring(0, cut) else url).lowercase(Locale.ROOT)
    return when {
        path.endsWith(".m3u8") -> Container.HLS
        path.endsWith(".mpd") -> Container.DASH
        path.endsWith(".mp4") -> Container.MP4
        else -> null
    }
}

/**
 * Port of `lang_codes`: lowercase ISO 639-1 (`"es-419"` -> `es`, `"EN"` -> `en`), dropping `und` / empty and duplicates, first-seen
 * order. Public twin in :core:release is `langCodes` (same body).
 */
fun normalizeLangCodes(tags: Iterable<String>): List<String> {
    val out = ArrayList<String>()
    for (t in tags) {
        val code = t.trim().split('-', '_').first().lowercase(Locale.ROOT)
        if (code.isEmpty() || code == "und" || code in out) continue
        out.add(code)
    }
    return out
}
