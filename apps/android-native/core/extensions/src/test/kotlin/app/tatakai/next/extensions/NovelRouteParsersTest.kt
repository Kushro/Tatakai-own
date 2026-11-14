// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** BUG-100: Toko 4.x reads novels through `novel/chapters` / `novel/content`, not through a `novel` custom source. */
class NovelRouteParsersTest {
    @Test
    fun tokosNovelChaptersThroughThePcsProgressiveAndDebugEnvelopes() {
        // PC server: { chapters: <debugNovelChapters answer>, partial }; the debug answer is { results, diagnostics, complete }.
        val body = Json.parseToJsonElement(
            """{"chapters":{"results":[{"number":1,"title":"Prologue","volume":null,"sources":[
                {"provider":"wuxiaworld","chapterKey":"wuxiaworld:abc","providerChapterId":"abc","language":"en","translator":"WW","releaseDate":null}]}],
                "diagnostics":[],"complete":true},"partial":false}""",
        )
        val list = ProviderParsers.mangaRouteChapters("tatakai.extension.toko", body)
        assertEquals(1, list.size)
        assertEquals("wuxiaworld", list[0].provider)
        assertEquals("wuxiaworld:abc", list[0].key)
        assertEquals("WW", list[0].group, "the translator")
    }

    @Test
    fun novelContentParagraphObjectsInIndexOrder() {
        val body = Json.parseToJsonElement(
            """{"title":"Ch 1","number":1,"provider":"wuxiaworld","paragraphs":[{"index":1,"text":" Second  one "},{"index":0,"text":"First"},{"index":2,"text":"  "}]}""",
        )
        assertEquals(listOf("First", "Second one"), ProviderParsers.novelContent(body).map { it.url })
        assertEquals(listOf("A", "B"), ProviderParsers.novelContent(Json.parseToJsonElement("""{"content":"<p>A</p><p>B</p>"}""")).map { it.url })
        assertEquals(0, ProviderParsers.novelContent(Json.parseToJsonElement("""{"error":"x"}""")).size)
    }

    @Test
    fun manifestsThatReadNovelsByRoute() {
        fun manifest(extra: String) = ManifestParser.parse(
            """{"id":"x","name":"X","type":"custom","version":"1","description":"","permissions":[]$extra}""",
        )
        assertTrue(manifest(""","capabilities":["manga","novel"],"apiServer":{"namespace":"x","routes":["novel/search","novel/chapters","novel/content"]}""").readsNovelsByRoute)
        assertFalse(manifest(""","capabilities":["manga"],"apiServer":{"namespace":"x","routes":["manga/chapters"]}""").readsNovelsByRoute)
        assertFalse(manifest(""","capabilities":["custom-source","novel"],"customSources":[{"id":"n","kind":"novel","name":"N"}]""").readsNovelsByRoute)
    }
}
