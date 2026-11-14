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
    ): List<MangaChapterSource> = chapters.groupBy { it.sourceId }.map { (sourceId, list) ->
        MangaChapterSource(
            extensionId = ref.extensionId,
            extensionName = sourceName(ref.extensionName, providerOf(sourceId)),
            sourceId = sourceId,
            itemId = mediaKey,
            languages = ChapterLists.languages(list),
            chapterCount = list.size,
            reliability = reliability(sourceKey(ref.extensionId, sourceId)) ?: INITIAL_RELIABILITY,
        )
    }

    /** "Toko · MangaDex", "Toko · eidos · MangaDex" (the PC's copy), like the anime sources. */
    fun sourceName(extensionName: String, provider: String): String {
        val label = providerLabel(provider)
        return if (label.equals(extensionName, ignoreCase = true)) extensionName else "$extensionName · $label"
    }

    /** Display name of an inner provider id (`mangadex` -> MangaDex); unknown ids are capitalized. */
    fun providerLabel(provider: String): String {
        val id = provider.trim()
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
