// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import java.text.Normalizer
import java.util.Locale

// parity wave A (docs/redesign/15 §3.2, bug `player.subs.preferred-lang`). Owner: worker p. Pure, no Android.
//
// Extensions and containers label tracks in every possible way: `es`, `es-419`, `es_ES`, `spa`, `Spanish`, `Español`, `Spanish (Latin
// America)`, `[Castellano]`... `SourcePrefs.subLang` / `dubLang` are ISO 639-1 (`es`). Media3 and mpv only compare the raw tags, so a
// preference of `es` never matched `es-419` / `spa` and the stream started with subtitles off. Everything here reduces a tag or a
// label to ISO 639-1 so the engines can pick the right track.

object LanguageCodes {
    private val COMBINING = Regex("\\p{Mn}+")

    /** Common mislabels of two-letter codes (country instead of language). */
    private val TWO_LETTER_FIXES: Map<String, String> = mapOf("jp" to "ja", "cn" to "zh", "kr" to "ko", "br" to "pt", "sp" to "es")

    /** ISO 639-2 (bibliographic and terminology) -> ISO 639-1, for the languages anime streams carry. */
    private val ISO3: Map<String, String> = mapOf(
        "spa" to "es", "esl" to "es", "eng" to "en", "jpn" to "ja", "jap" to "ja", "por" to "pt", "fre" to "fr", "fra" to "fr",
        "ger" to "de", "deu" to "de", "ita" to "it", "rus" to "ru", "kor" to "ko", "chi" to "zh", "zho" to "zh", "cmn" to "zh",
        "yue" to "zh", "ara" to "ar", "hin" to "hi", "ind" to "id", "may" to "ms", "msa" to "ms", "vie" to "vi", "tha" to "th",
        "tur" to "tr", "pol" to "pl", "dut" to "nl", "nld" to "nl", "swe" to "sv", "nor" to "no", "nob" to "no", "dan" to "da",
        "fin" to "fi", "cze" to "cs", "ces" to "cs", "slo" to "sk", "slk" to "sk", "rum" to "ro", "ron" to "ro", "hun" to "hu",
        "gre" to "el", "ell" to "el", "heb" to "he", "per" to "fa", "fas" to "fa", "ukr" to "uk", "bul" to "bg", "srp" to "sr",
        "hrv" to "hr", "cat" to "ca", "fil" to "tl", "tgl" to "tl", "tam" to "ta", "tel" to "te", "mal" to "ml", "ben" to "bn",
        "urd" to "ur", "glg" to "gl", "baq" to "eu", "eus" to "eu",
    )

    /** Language names (English, the language itself, Spanish), accent-free and lowercase -> ISO 639-1. */
    private val NAMES: Map<String, String> = mapOf(
        "spanish" to "es", "espanol" to "es", "castellano" to "es", "latino" to "es", "latin american spanish" to "es",
        "english" to "en", "ingles" to "en", "japanese" to "ja", "japones" to "ja", "nihongo" to "ja", "日本語" to "ja",
        "portuguese" to "pt", "portugues" to "pt", "brazilian portuguese" to "pt", "french" to "fr", "francais" to "fr",
        "frances" to "fr", "german" to "de", "deutsch" to "de", "aleman" to "de", "italian" to "it", "italiano" to "it",
        "russian" to "ru", "русский" to "ru", "ruso" to "ru", "korean" to "ko", "coreano" to "ko", "한국어" to "ko",
        "chinese" to "zh", "chino" to "zh", "中文" to "zh", "arabic" to "ar", "arabe" to "ar", "العربية" to "ar",
        "hindi" to "hi", "indonesian" to "id", "bahasa indonesia" to "id", "malay" to "ms", "vietnamese" to "vi", "thai" to "th",
        "turkish" to "tr", "turkce" to "tr", "polish" to "pl", "polski" to "pl", "dutch" to "nl", "swedish" to "sv",
        "norwegian" to "no", "danish" to "da", "finnish" to "fi", "czech" to "cs", "romanian" to "ro", "hungarian" to "hu",
        "greek" to "el", "hebrew" to "he", "persian" to "fa", "ukrainian" to "uk", "catalan" to "ca", "catala" to "ca",
        "filipino" to "tl", "tagalog" to "tl",
    ).mapKeys { fold(it.key) }

    /**
     * ISO 639-1 code of a language tag or a human label, or null when nothing can be recognised. `es-419` / `es_ES` / `spa` /
     * `Spanish` / `Español (Latinoamérica)` -> `es`. `und` / empty -> null.
     */
    fun normalize(tagOrLabel: String?): String? {
        val raw = tagOrLabel?.trim()?.takeIf { it.isNotEmpty() } ?: return null
        val folded = fold(raw)
        NAMES[folded]?.let { return it }
        val head = folded.split('-', '_', ' ', '(', ')', '[', ']', '.', ',', '/', ':').firstOrNull { it.isNotEmpty() } ?: return null
        if (head == "und" || head == "unk" || head == "mul" || head == "zxx") return null
        if (head.length == 2 && head.all { it in 'a'..'z' }) return TWO_LETTER_FIXES[head] ?: head
        if (head.length == 3 && head.all { it in 'a'..'z' }) ISO3[head]?.let { return it }
        NAMES[head]?.let { return it }
        // "Spanish (Latin America)", "[Español] Signs & Songs": any known name inside the label.
        return NAMES.entries.firstOrNull { (name, _) -> name.length > 3 && folded.contains(name) }?.value
    }

    /** True when a track with [lang] / [label] is in the [preferred] language (ISO 639-1). */
    fun matches(lang: String?, label: String?, preferred: String?): Boolean {
        val want = normalize(preferred) ?: return false
        return normalize(lang) == want || normalize(label) == want
    }

    /**
     * Tags to hand to an engine for [preferred] (`es`): the regional variants first, then the bare code, then ISO 639-2, so Media3's
     * `setPreferredTextLanguages` and mpv's `slang` both match whatever the stream uses.
     */
    fun engineVariants(preferred: String?): List<String> {
        val code = normalize(preferred) ?: return emptyList()
        val regional = REGIONAL[code].orEmpty()
        val iso3 = ISO3.filterValues { it == code }.keys.sorted()
        return (regional + code + iso3).distinct()
    }

    /**
     * Index of the first track in [preferred] (by [lang] / [label] of each element), or null. Used to force a selection when the engine
     * started with every subtitle track off although one matches.
     */
    fun <T> firstMatch(tracks: List<T>, preferred: String?, lang: (T) -> String?, label: (T) -> String?): Int? {
        if (normalize(preferred) == null) return null
        val i = tracks.indexOfFirst { matches(lang(it), label(it), preferred) }
        return i.takeIf { it >= 0 }
    }

    private val REGIONAL: Map<String, List<String>> = mapOf(
        "es" to listOf("es-419", "es-ES", "es-MX"),
        "pt" to listOf("pt-BR", "pt-PT"),
        "en" to listOf("en-US", "en-GB"),
        "zh" to listOf("zh-Hans", "zh-Hant", "zh-CN", "zh-TW"),
    )

    private fun fold(s: String): String =
        Normalizer.normalize(s.lowercase(Locale.ROOT), Normalizer.Form.NFD).replace(COMBINING, "").trim()
}
