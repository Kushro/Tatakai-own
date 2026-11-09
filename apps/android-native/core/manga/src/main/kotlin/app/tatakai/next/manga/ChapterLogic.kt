// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import java.text.Normalizer
import java.util.Locale
import kotlin.math.abs

// Pure chapter / source logic of the readers (classic MangaChapterProviderRows + MangaReaderPage helpers), shared by :core:manga,
// :core:novel and the screens. Everything here is JVM-testable.

/** `<extensionId>/<sourceId>`: the key of a source in preferences and reliability stats. */
fun sourceKey(extensionId: String, sourceId: String): String = "$extensionId/$sourceId"

val MangaChapterSource.key: String get() = sourceKey(extensionId, sourceId)

val MangaChapter.sourceKey: String get() = sourceKey(extensionId, sourceId)

object TitleMatch {
    /** Lowercase, accents and punctuation removed, single spaces. */
    fun normalize(title: String): String {
        val decomposed = Normalizer.normalize(title, Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")
        return decomposed.lowercase(Locale.ROOT)
            .replace(Regex("[^\\p{L}\\p{N}]+"), " ")
            .trim()
            .replace(Regex("\\s+"), " ")
    }

    /** Sørensen–Dice coefficient over character bigrams of the normalized titles, 0..1 (1 = same title). */
    fun similarity(a: String, b: String): Double {
        val x = normalize(a).replace(" ", "")
        val y = normalize(b).replace(" ", "")
        if (x.isEmpty() || y.isEmpty()) return 0.0
        if (x == y) return 1.0
        if (x.length < 2 || y.length < 2) return 0.0
        val bx = x.windowed(2).groupingBy { it }.eachCount().toMutableMap()
        var hits = 0
        y.windowed(2).forEach { g ->
            val n = bx[g] ?: 0
            if (n > 0) {
                hits++
                bx[g] = n - 1
            }
        }
        return 2.0 * hits / ((x.length - 1) + (y.length - 1))
    }

    /** The search result whose title best matches any of [titles]; null under [threshold]. */
    fun best(items: List<ExtensionHomeItem>, titles: List<String>, threshold: Double = MATCH_THRESHOLD): ExtensionHomeItem? {
        val wanted = titles.filter { it.isNotBlank() }.distinct()
        if (wanted.isEmpty()) return null
        return items
            .map { item -> item to wanted.maxOf { similarity(it, item.title) } }
            .filter { it.second >= threshold }
            .maxByOrNull { it.second }
            ?.first
    }

    const val MATCH_THRESHOLD: Double = 0.6
}

object ChapterText {
    private val VOLUME = Regex("(?i)\\b(?:vol(?:ume)?|tomo|v)\\.?\\s*(\\d{1,4})")
    private val NUMBER = Regex("(?i)(?:\\bch(?:apter)?|\\bcap(?:[ií]tulo)?|\\bep(?:isode)?|#)\\.?\\s*(\\d+(?:[.,]\\d+)?)")
    private val LEADING_NUMBER = Regex("^\\s*(\\d+(?:\\.\\d+)?)\\b")
    private val GROUP = Regex("^\\s*\\[([^\\]]{1,60})]")

    fun volume(title: String?): Int? = title?.let { VOLUME.find(it)?.groupValues?.get(1)?.toIntOrNull() }

    fun number(title: String?): Double? {
        if (title.isNullOrBlank()) return null
        NUMBER.find(title)?.groupValues?.get(1)?.replace(',', '.')?.toDoubleOrNull()?.let { return it }
        return LEADING_NUMBER.find(title)?.groupValues?.get(1)?.toDoubleOrNull()
    }

    fun group(title: String?): String? = title?.let { GROUP.find(it)?.groupValues?.get(1)?.trim()?.takeIf { g -> g.isNotEmpty() } }

    /** "12", "12.5" (no trailing `.0`). */
    fun formatNumber(number: Double?): String? = number?.let {
        if (it == Math.floor(it) && !it.isInfinite()) it.toLong().toString() else it.toString()
    }
}

enum class ChapterOrder { ASC, DESC }

object ChapterLists {
    /** Ascending by number (unknown numbers last, then by release time), or the reverse. */
    fun sorted(chapters: List<MangaChapter>, order: ChapterOrder): List<MangaChapter> {
        val asc = chapters.sortedWith(
            compareBy<MangaChapter>({ it.number == null }, { it.number ?: Double.MAX_VALUE }, { it.releasedAtMs ?: Long.MAX_VALUE }),
        )
        return if (order == ChapterOrder.ASC) asc else asc.reversed()
    }

    /** Text filter over number, title and group; [lang] keeps one language (null = all). */
    fun filtered(chapters: List<MangaChapter>, query: String, lang: String?): List<MangaChapter> {
        val q = query.trim().lowercase(Locale.ROOT)
        return chapters.filter { ch ->
            (lang == null || ch.lang.equals(lang, ignoreCase = true)) &&
                (
                    q.isEmpty() ||
                        ChapterText.formatNumber(ch.number)?.startsWith(q) == true ||
                        ch.title?.lowercase(Locale.ROOT)?.contains(q) == true ||
                        ch.group?.lowercase(Locale.ROOT)?.contains(q) == true
                    )
        }
    }

    /** Groups by volume keeping the order of [chapters]; chapters without volume go to a null group at the end. */
    fun byVolume(chapters: List<MangaChapter>): List<Pair<Int?, List<MangaChapter>>> {
        val groups = LinkedHashMap<Int?, MutableList<MangaChapter>>()
        chapters.forEach { groups.getOrPut(it.volume) { mutableListOf() } += it }
        val out = ArrayList<Pair<Int?, List<MangaChapter>>>()
        groups.forEach { (volume, list) -> if (volume != null) out.add(volume to list.toList()) }
        groups[null]?.let { out.add(null to it.toList()) }
        return out
    }

    fun languages(chapters: List<MangaChapter>): List<String> = chapters.mapNotNull { it.lang?.lowercase(Locale.ROOT) }.distinct().sorted()

    /** Next / previous chapter by number in the same list (the reader's "siguiente capítulo"). */
    fun neighbour(chapters: List<MangaChapter>, current: MangaChapter, forward: Boolean): MangaChapter? {
        val asc = sorted(chapters.filter { it.lang == null || current.lang == null || it.lang == current.lang }, ChapterOrder.ASC)
            .distinctBy { it.number ?: it.id.hashCode().toDouble() }
        val index = asc.indexOfFirst { it.id == current.id }.takeIf { it >= 0 }
            ?: asc.indexOfFirst { it.number != null && it.number == current.number }.takeIf { it >= 0 }
            ?: return null
        return asc.getOrNull(if (forward) index + 1 else index - 1)
    }

    /** Highest chapter number of a list (followed updates). */
    fun latestNumber(chapters: List<MangaChapter>): Double? = chapters.mapNotNull { it.number }.maxOrNull()
}

/**
 * Picks sources and chapters (classic auto-best): a preferred source wins, then the preferred languages in order, then reliability.
 * [preferredLanguages] are ISO 639-1 codes, best first.
 */
object SourcePicker {
    fun score(source: MangaChapterSource, preferredLanguages: List<String>, preferredSource: String?): Double {
        var s = source.reliability.toDouble().coerceIn(0.0, 1.0)
        if (preferredSource != null && source.key == preferredSource) s += 10.0
        val langIndex = preferredLanguages.indexOfFirst { pref -> source.languages.any { it.equals(pref, ignoreCase = true) } }
        if (langIndex >= 0) s += 2.0 * (preferredLanguages.size - langIndex) / preferredLanguages.size.coerceAtLeast(1)
        if (source.languages.isEmpty()) s += 0.25
        if (source.itemId == null) s -= 5.0
        return s
    }

    fun rank(sources: List<MangaChapterSource>, preferredLanguages: List<String>, preferredSource: String?): List<MangaChapterSource> =
        sources.sortedByDescending { score(it, preferredLanguages, preferredSource) }

    /**
     * The best chapter numbered [number] among [candidates] (one list per source): preferred source, then language, then source
     * reliability, then the newest release.
     */
    fun bestChapter(
        candidates: List<Pair<MangaChapterSource, List<MangaChapter>>>,
        number: Double,
        preferredLanguages: List<String>,
        preferredSource: String?,
    ): MangaChapter? {
        val hits = candidates.flatMap { (source, chapters) ->
            chapters.filter { it.number != null && abs(it.number!! - number) < 0.001 }.map { source to it }
        }
        if (hits.isEmpty()) return null
        return hits.maxWithOrNull(
            compareBy<Pair<MangaChapterSource, MangaChapter>>(
                { (source, _) -> if (preferredSource != null && source.key == preferredSource) 1 else 0 },
                { (_, ch) -> langRank(ch.lang, preferredLanguages) },
                { (source, _) -> source.reliability },
                { (_, ch) -> ch.releasedAtMs ?: 0L },
            ),
        )?.second
    }

    private fun langRank(lang: String?, preferred: List<String>): Int {
        if (lang == null) return 0
        val i = preferred.indexOfFirst { it.equals(lang, ignoreCase = true) }
        return if (i < 0) -1 else preferred.size - i
    }

    /** Moving average used for [MangaChapterSource.reliability]: 80 % history, 20 % the last result. */
    fun updateReliability(current: Float?, ok: Boolean): Float {
        val base = current ?: INITIAL_RELIABILITY
        return (base * 0.8f + (if (ok) 1f else 0f) * 0.2f).coerceIn(0f, 1f)
    }

    /** App language first, then English, then the user's extra languages. */
    fun languages(appLanguage: String?, extra: List<String>): List<String> =
        (extra + listOfNotNull(appLanguage) + "en").map { it.lowercase(Locale.ROOT) }.distinct()
}
