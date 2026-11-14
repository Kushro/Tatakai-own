// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaRouteChapter
import java.util.Locale

/**
 * BUG-099: route-based manga extensions (`manga/chapters`, Toko 4.x and the Spanish pack) as reader sources. One `manga/chapters` call
 * answers every inner provider of the extension (MangaDex, Comick, Mangapill…); like desktop (`<ext_id>/<provider>`), each provider is a
 * source of its own, `sourceId = "manga-route:<provider>"`, and the title's media key (`"30013"`, `"mal:5"`) is its `itemId` (the
 * extension finds the title by AniList id and titles, there is no per-source search). Pure, JVM-tested.
 */
object MangaRoutes {
    const val SOURCE_PREFIX: String = "manga-route:"

    fun isRouteSource(sourceId: String): Boolean = sourceId.startsWith(SOURCE_PREFIX)

    fun sourceIdOf(provider: String): String = SOURCE_PREFIX + provider

    fun providerOf(sourceId: String): String = sourceId.removePrefix(SOURCE_PREFIX)

    /** The chapters of one `manga/chapters` answer, as reader chapters of [extensionId] for the title [mediaKey]. */
    fun chapters(extensionId: String, mediaKey: String, answer: List<MangaRouteChapter>): List<MangaChapter> = answer.map { c ->
        MangaChapter(
            id = c.key,
            extensionId = extensionId,
            sourceId = sourceIdOf(c.provider),
            itemId = mediaKey,
            number = c.number ?: ChapterText.number(c.title),
            volume = c.volume ?: ChapterText.volume(c.title),
            title = c.title,
            lang = c.lang,
            group = c.group,
            releasedAtMs = c.releasedAtMs,
        )
    }

    /** One source per inner provider that has chapters, in the order the extension answered. */
    fun sources(
        ref: CustomSourceRef,
        mediaKey: String,
        chapters: List<MangaChapter>,
        reliability: (sourceKey: String) -> Float?,
    ): List<MangaChapterSource> {
        val groups = chapters.groupBy { it.sourceId }.toList()
        val names = distinctNames(
            groups.map { (sourceId, _) -> sourceName(ref.extensionName, providerOf(sourceId)) },
            groups.map { (_, list) -> list.mapNotNull { it.group?.trim()?.takeIf(String::isNotEmpty) }.distinct().singleOrNull() },
        )
        return groups.mapIndexed { i, (sourceId, list) ->
            MangaChapterSource(
                extensionId = ref.extensionId,
                extensionName = names[i],
                sourceId = sourceId,
                itemId = mediaKey,
                languages = ChapterLists.languages(list),
                chapterCount = list.size,
                reliability = reliability(sourceKey(ref.extensionId, sourceId)) ?: INITIAL_RELIABILITY,
            )
        }
    }

    /**
     * BUG-123: two providers of one extension can share a name once the instance id is gone ("Toko · Atsu Novel" twice): they get
     * their only group/translator ("Toko · Atsu Novel · Red") or, without one, a number ("Toko · Atsu Novel #2").
     */
    fun distinctNames(names: List<String>, extras: List<String?>): List<String> {
        val counts = names.groupingBy { it }.eachCount()
        val withExtra = names.mapIndexed { i, n ->
            val extra = extras.getOrNull(i)
            if (counts.getValue(n) > 1 && extra != null) "$n · $extra" else n
        }
        val seen = HashMap<String, Int>()
        val again = withExtra.groupingBy { it }.eachCount()
        return withExtra.map { n ->
            if (again.getValue(n) < 2) n else {
                val k = seen.merge(n, 1, Int::plus)!!
                if (k == 1) n else "$n #$k"
            }
        }
    }

    /** "Toko · MangaDex", "Toko · eidos · MangaDex" (the PC's copy), like the anime sources. */
    fun sourceName(extensionName: String, provider: String): String {
        val label = providerLabel(provider)
        return if (label.equals(extensionName, ignoreCase = true)) extensionName else "$extensionName · $label"
    }

    /**
     * Display name of an inner provider id (`mangadex` -> MangaDex); unknown ids are capitalized. BUG-123: Toko names some providers
     * `atsu@<instance id>`: the instance id is not part of the name ("Atsu"), as on the desktop (BUG-119).
     */
    fun providerLabel(provider: String): String {
        val id = provider.trim().substringBefore('@').trim()
        KNOWN[id.lowercase(Locale.ROOT)]?.let { return it }
        return id.split('-', '_', ' ').filter { it.isNotEmpty() }
            .joinToString(" ") { part -> part.replaceFirstChar { c -> c.titlecase(Locale.ROOT) } }
            .ifEmpty { id }
    }

    private val KNOWN = mapOf(
        "mangadex" to "MangaDex",
        "comick" to "Comick",
        "mangapill" to "Mangapill",
        "mangakatana" to "MangaKatana",
        "weebcentral" to "Weeb Central",
        "nelomanga" to "NeloManga",
        "atsu" to "Atsu",
        "atsu-novel" to "Atsu Novel",
        "webtoons" to "Webtoons",
        "demonicscans" to "Demonic Scans",
        "toongod" to "ToonGod",
        "toonily" to "Toonily",
        "onisaga" to "OniSaga",
        "mgeko" to "Mgeko",
        "scansgg" to "Scans.gg",
        "leercapitulo" to "LeerCapitulo",
        "novelcool" to "NovelCool",
    )
}
