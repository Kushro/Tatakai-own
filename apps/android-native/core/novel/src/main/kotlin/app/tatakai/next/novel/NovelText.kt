// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.TitleMatch
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.NovelChapter
import org.jsoup.Jsoup
import org.jsoup.nodes.Document
import java.util.Locale

// Pure text logic of the novel reader (classic NovelReaderPage + src/lib/novel helpers): HTML cleaning, paragraph splitting, chapter
// sorting / filtering by translator, narration chunks. JVM-testable (jsoup is plain Java).

object NovelText {
    private val BLOCK_SPLIT = Regex("\\n\\s*\\n|\\r\\n\\s*\\r\\n")
    private val LINE_SPLIT = Regex("\\r?\\n")
    private val TAG = Regex("<\\s*(p|br|div|span|em|strong|i|b|h[1-6]|li|ul|ol|blockquote|section|article)\\b", RegexOption.IGNORE_CASE)
    private val NOISE_SELECTORS = "script, style, noscript, iframe, ins, form, button, nav, footer, header, " +
        ".ads, .ad, .adsbygoogle, [id^=ad-], [class*=advert], .sharedaddy, .comments, #comments"
    private val NOISE_LINES = listOf(
        Regex("(?i)^\\s*(previous|next)\\s+chapter\\s*$"),
        Regex("(?i)^\\s*(cap[ií]tulo\\s+)?(anterior|siguiente)\\s*$"),
        Regex("(?i)^\\s*table of contents\\s*$"),
        Regex("(?i)^\\s*(please )?(support|read) (us|this) (at|on) .{0,40}$"),
    )

    fun looksLikeHtml(text: String): Boolean = TAG.containsMatchIn(text)

    fun looksLikeUrl(text: String): Boolean {
        val t = text.trim()
        return (t.startsWith("http://") || t.startsWith("https://")) && t.none { it.isWhitespace() }
    }

    /** Paragraphs of an HTML chapter body: ads / scripts removed, `<p>` (or `<br>`-separated text) one per paragraph. */
    fun paragraphsFromHtml(html: String): List<String> {
        val doc: Document = Jsoup.parse(html)
        doc.select(NOISE_SELECTORS).remove()
        val root = doc.selectFirst("#chapter-content, .chapter-content, #chr-content, .text-left, .reading-content, .entry-content, article")
            ?: doc.body()
        val ps = root.select("p")
        val out = if (ps.size >= 2) {
            ps.map { it.text() }
        } else {
            root.select("br").forEach { it.after("\n") }
            root.select("div, h1, h2, h3, h4, li").forEach { it.prepend("\n\n") }
            splitPlain(root.wholeText())
        }
        return clean(out)
    }

    /** Plain text split on blank lines; a text without blank lines is split per line. */
    fun splitPlain(text: String): List<String> {
        val normalized = text.replace(' ', ' ')
        val blocks = normalized.split(BLOCK_SPLIT)
        val parts = if (blocks.size > 1) blocks else normalized.split(LINE_SPLIT)
        return clean(parts.map { it.replace(LINE_SPLIT, " ") })
    }

    /** Trims, collapses spaces, drops empty and navigation lines. */
    fun clean(paragraphs: List<String>): List<String> = paragraphs
        .map { it.replace(Regex("[ \\t\\u00A0]+"), " ").trim() }
        .filter { p -> p.isNotEmpty() && NOISE_LINES.none { it.matches(p) } }

    /**
     * Paragraphs of a `custom/read` answer of a `novel` source: every [ChapterPage.url] carries text (one paragraph, a block of text or
     * a whole HTML body). A page that is only a URL is fetched with [fetch] (its own headers) and parsed as HTML.
     */
    suspend fun fromPages(pages: List<ChapterPage>, fetch: suspend (ChapterPage) -> String): List<String> {
        val out = ArrayList<String>()
        pages.sortedBy { it.index }.forEach { page ->
            val raw = page.url
            when {
                looksLikeUrl(raw) -> out += paragraphsFromHtml(fetch(page))
                raw.startsWith("data:text/html", ignoreCase = true) -> out += paragraphsFromHtml(raw.substringAfter(','))
                looksLikeHtml(raw) -> out += paragraphsFromHtml(raw)
                else -> out += splitPlain(raw)
            }
        }
        return out
    }

    /**
     * Pieces of one paragraph no longer than [max] characters (Android `TextToSpeech.getMaxSpeechInputLength()` is 4000), cut at sentence
     * ends when possible, else at spaces.
     */
    fun speechChunks(paragraph: String, max: Int = 3900): List<String> {
        val text = paragraph.trim()
        if (text.isEmpty()) return emptyList()
        if (text.length <= max) return listOf(text)
        val out = ArrayList<String>()
        var rest = text
        while (rest.length > max) {
            val window = rest.substring(0, max)
            val cut = listOf(". ", "! ", "? ", "。", "」", "; ").maxOf { window.lastIndexOf(it).let { i -> if (i < 0) -1 else i + it.length } }
                .takeIf { it > max / 3 }
                ?: window.lastIndexOf(' ').takeIf { it > max / 3 }
                ?: max
            out += rest.substring(0, cut).trim()
            rest = rest.substring(cut).trim()
        }
        if (rest.isNotEmpty()) out += rest
        return out
    }

    /** Words of a chapter (reading time estimate of the reader chrome). */
    fun wordCount(paragraphs: List<String>): Int = paragraphs.sumOf { p -> p.split(Regex("\\s+")).count { it.isNotBlank() } }
}

object NovelChapters {
    fun sorted(chapters: List<NovelChapter>, ascending: Boolean): List<NovelChapter> {
        val asc = chapters.sortedWith(
            compareBy<NovelChapter>({ it.number == null }, { it.number ?: Double.MAX_VALUE }, { it.releasedAtMs ?: Long.MAX_VALUE }),
        )
        return if (ascending) asc else asc.reversed()
    }

    fun translators(chapters: List<NovelChapter>): List<String> = chapters.mapNotNull { it.translator?.trim()?.takeIf { t -> t.isNotEmpty() } }.distinct().sorted()

    fun languages(chapters: List<NovelChapter>): List<String> = chapters.mapNotNull { it.lang?.lowercase(Locale.ROOT) }.distinct().sorted()

    /** Text filter over number and title; [translator] / [lang] keep one value (null = all). */
    fun filtered(chapters: List<NovelChapter>, query: String, translator: String?, lang: String?): List<NovelChapter> {
        val q = query.trim().lowercase(Locale.ROOT)
        return chapters.filter { ch ->
            (translator == null || ch.translator == translator) &&
                (lang == null || ch.lang.equals(lang, ignoreCase = true)) &&
                (q.isEmpty() || ChapterText.formatNumber(ch.number)?.startsWith(q) == true || ch.title?.lowercase(Locale.ROOT)?.contains(q) == true)
        }
    }

    fun neighbour(chapters: List<NovelChapter>, currentId: String, forward: Boolean): NovelChapter? {
        val asc = sorted(chapters, ascending = true)
        val i = asc.indexOfFirst { it.id == currentId }
        if (i < 0) return null
        return asc.getOrNull(if (forward) i + 1 else i - 1)
    }

    /** Chapters of one source deduplicated by number (several translators): the one of [preferredTranslator] wins. */
    fun dedupe(chapters: List<NovelChapter>, preferredTranslator: String?): List<NovelChapter> =
        chapters.groupBy { it.number ?: it.id.hashCode().toDouble() }.values.map { group ->
            group.firstOrNull { preferredTranslator != null && it.translator == preferredTranslator } ?: group.first()
        }

    fun titleScore(a: String, b: String): Double = TitleMatch.similarity(a, b)
}
