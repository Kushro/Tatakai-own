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
    fun sources(ref: CustomSourceRef, mediaKey: String, chapters: List<NovelChapter>): List<NovelSourceSummary> =
        chapters.groupBy { it.sourceId }.map { (sourceId, list) ->
            NovelSourceSummary(
                extensionId = ref.extensionId,
                extensionName = MangaRoutes.sourceName(ref.extensionName, providerOf(sourceId)),
                sourceId = sourceId,
                itemId = mediaKey,
                languages = list.mapNotNull { it.lang }.distinct().sorted(),
                chapterCount = list.size,
                translator = list.mapNotNull { it.translator }.distinct().singleOrNull(),
            )
        }
}
