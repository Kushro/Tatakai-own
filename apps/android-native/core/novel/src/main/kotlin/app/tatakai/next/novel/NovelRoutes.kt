// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.manga.MangaRoutes
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.MangaRouteChapter
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelSourceSummary
import java.util.Locale

/**
 * BUG-100: route-based novel extensions (`novel/chapters` / `novel/content`, Toko 4.x) as reader sources, like the manga ones
 * ([MangaRoutes], BUG-099): one source per inner provider (`sourceId = "novel-route:<provider>"`), the title's media key as `itemId`,
 * the `chapterKey` as chapter id. Pure, JVM-tested.
 */
object NovelRoutes {
    const val SOURCE_PREFIX: String = "novel-route:"

    fun isRouteSource(sourceId: String): Boolean = sourceId.startsWith(SOURCE_PREFIX)

    fun providerOf(sourceId: String): String = sourceId.removePrefix(SOURCE_PREFIX)

    /**
     * BUG-100: the titles sent to the extensions, in the desktop order (`tk_core::novel::anilist`): English, romaji, the title shown,
     * native, then synonyms. Toko searches its English catalogues with only the first two, so the English one goes first.
     */
    fun searchTitles(shown: String, english: String?, romaji: String?, native: String?, synonyms: List<String>): List<String> =
        (listOfNotNull(english, romaji, shown, native) + synonyms.take(3)).map { it.trim() }.filter { it.isNotEmpty() }.distinct()

    fun chapters(extensionId: String, mediaKey: String, answer: List<MangaRouteChapter>): List<NovelChapter> = answer.map { c ->
        NovelChapter(
            id = c.key,
            extensionId = extensionId,
            sourceId = SOURCE_PREFIX + c.provider,
            itemId = mediaKey,
            number = c.number,
            title = c.title,
            lang = c.lang?.lowercase(Locale.ROOT),
            translator = c.group,
            releasedAtMs = c.releasedAtMs,
        )
    }

    /** One source per provider that has chapters ("Toko · Wuxiaworld"), in the order the extension answered. */
    fun sources(ref: CustomSourceRef, mediaKey: String, chapters: List<NovelChapter>): List<NovelSourceSummary> {
        val groups = chapters.groupBy { it.sourceId }.toList()
        val translators = groups.map { (_, list) -> list.mapNotNull { it.translator?.trim()?.takeIf(String::isNotEmpty) }.distinct().singleOrNull() }
        // BUG-123: "Toko · Atsu Novel" (no instance id); two of them become "… · Red" / "… · Red 2" (or "#2").
        val names = MangaRoutes.distinctNames(groups.map { (sourceId, _) -> MangaRoutes.sourceName(ref.extensionName, providerOf(sourceId)) }, translators)
        return groups.mapIndexed { i, (sourceId, list) ->
            NovelSourceSummary(
                extensionId = ref.extensionId,
                extensionName = names[i],
                sourceId = sourceId,
                itemId = mediaKey,
                languages = list.mapNotNull { it.lang }.distinct().sorted(),
                chapterCount = list.size,
                translator = translators[i],
            )
        }
    }
}
