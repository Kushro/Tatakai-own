// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReaderSettings
import app.tatakai.next.model.ReadingProgress
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ChapterLogicTest {
    private fun ch(id: String, n: Double?, lang: String? = "en", vol: Int? = null, src: String = "s", ext: String = "e", group: String? = null, at: Long? = null) =
        MangaChapter(id = id, extensionId = ext, sourceId = src, itemId = "i", number = n, volume = vol, lang = lang, group = group, releasedAtMs = at)

    private fun source(ext: String, langs: List<String>, rel: Float, item: String? = "i") =
        MangaChapterSource(ext, ext, "s", item, langs, null, rel)

    @Test
    fun chapterTextParsing() {
        assertEquals(12.5, ChapterText.number("Chapter 12.5: Rain")!!, 0.0)
        assertEquals(3.0, ChapterText.number("Capítulo 3")!!, 0.0)
        assertEquals(7.0, ChapterText.number("7 - The end")!!, 0.0)
        assertNull(ChapterText.number("Extra"))
        assertEquals(4, ChapterText.volume("Vol.4 Ch.20"))
        assertEquals("Evil Genius", ChapterText.group("[Evil Genius] Ch. 1"))
        assertEquals("12", ChapterText.formatNumber(12.0))
        assertEquals("12.5", ChapterText.formatNumber(12.5))
    }

    @Test
    fun sortFilterGroup() {
        val list = listOf(ch("b", 2.0, vol = 1), ch("x", null), ch("a", 1.0, vol = 1), ch("c", 3.0, vol = 2, lang = "es", group = "Lector"))
        assertEquals(listOf("a", "b", "c", "x"), ChapterLists.sorted(list, ChapterOrder.ASC).map { it.id })
        assertEquals(listOf("x", "c", "b", "a"), ChapterLists.sorted(list, ChapterOrder.DESC).map { it.id })
        assertEquals(listOf("c"), ChapterLists.filtered(list, "lect", null).map { it.id })
        assertEquals(listOf("c"), ChapterLists.filtered(list, "", "es").map { it.id })
        assertEquals(listOf("b"), ChapterLists.filtered(list, "2", null).map { it.id })
        val groups = ChapterLists.byVolume(ChapterLists.sorted(list, ChapterOrder.ASC))
        assertEquals(listOf(1, 2, null), groups.map { it.first })
        assertEquals(listOf("en", "es"), ChapterLists.languages(list))
    }

    @Test
    fun neighbourChapters() {
        val list = listOf(ch("a", 1.0), ch("b", 2.0), ch("c", 3.0), ch("b-es", 2.0, lang = "es"))
        assertEquals("c", ChapterLists.neighbour(list, list[1], forward = true)?.id)
        assertEquals("a", ChapterLists.neighbour(list, list[1], forward = false)?.id)
        assertNull(ChapterLists.neighbour(list, list[2], forward = true))
    }

    @Test
    fun sourceRankingPrefersSourceThenLanguageThenReliability() {
        val en = source("en-src", listOf("en"), 0.5f)
        val es = source("es-src", listOf("es"), 0.9f)
        val unmatched = source("none", listOf("es"), 1f, item = null)
        assertEquals(listOf("es-src", "en-src", "none"), SourcePicker.rank(listOf(en, es, unmatched), listOf("es", "en"), null).map { it.extensionId })
        assertEquals("en-src", SourcePicker.rank(listOf(en, es), listOf("es", "en"), "en-src/s").first().extensionId)
    }

    @Test
    fun bestChapterByLanguageAndReliability() {
        val good = source("good", listOf("en", "es"), 0.95f)
        val bad = source("bad", listOf("en"), 0.2f)
        val candidates = listOf(
            bad to listOf(ch("1-bad", 5.0, ext = "bad")),
            good to listOf(ch("1-good-en", 5.0, ext = "good"), ch("1-good-es", 5.0, ext = "good", lang = "es")),
        )
        assertEquals("1-good-es", SourcePicker.bestChapter(candidates, 5.0, listOf("es", "en"), null)?.id)
        assertEquals("1-good-en", SourcePicker.bestChapter(candidates, 5.0, listOf("en"), null)?.id)
        assertEquals("1-bad", SourcePicker.bestChapter(candidates, 5.0, listOf("en"), "bad/s")?.id)
        assertNull(SourcePicker.bestChapter(candidates, 6.0, listOf("en"), null))
    }

    @Test
    fun reliabilityMovingAverage() {
        val down = SourcePicker.updateReliability(1f, ok = false)
        assertEquals(0.8f, down, 0.0001f)
        assertTrue(SourcePicker.updateReliability(down, ok = true) > down)
        assertEquals(listOf("pt", "es", "en"), SourcePicker.languages("es", listOf("pt")))
    }

    @Test
    fun titleMatching() {
        assertEquals(1.0, TitleMatch.similarity("Berserk!", "berserk"), 0.0)
        assertTrue(TitleMatch.similarity("Shingeki no Kyojin", "Shingeki no Kyoujin") > 0.8)
        val items = listOf(ExtensionHomeItem("1", "One Piece", null), ExtensionHomeItem("2", "Berserk", null))
        assertEquals("2", TitleMatch.best(items, listOf("Berserk"))?.id)
        assertNull(TitleMatch.best(items, listOf("Naruto")))
    }

    @Test
    fun readerSettingsSanitizedAndProgressCompleted() {
        val s = ReaderSettings(maxWidthDp = -3, pageGapDp = 999, background = "pink", brightness = 3f, prefetchPages = 0, autoScrollSpeed = 99999).sanitized()
        assertEquals(0, s.maxWidthDp)
        assertEquals(64, s.pageGapDp)
        assertEquals("black", s.background)
        assertEquals(1f, s.brightness, 0f)
        assertEquals(1, s.prefetchPages)
        assertEquals(2000, s.autoScrollSpeed)
        assertEquals(-1f, ReaderSettings(brightness = Float.NaN).sanitized().brightness, 0f)
        val p = ReadingProgress("1", "t", null, "c", 1.0, "e", "s", "i", page = 9, totalPages = 10, updatedAtMs = 0)
        assertTrue(p.completed)
        assertFalse(p.copy(page = 5).completed)
        assertFalse(p.copy(totalPages = 0).completed)
    }

    @Test
    fun keymapBindAndConflicts() {
        assertEquals("DPAD_RIGHT", ReaderKeymap.resolve(emptyMap())[ReaderKeymap.NEXT_PAGE])
        val bound = ReaderKeymap.bind(emptyMap(), ReaderKeymap.NEXT_PAGE, "N")
        assertEquals(ReaderKeymap.NEXT_PAGE, ReaderKeymap.actionFor(bound, "N"))
        assertTrue("N" in ReaderKeymap.conflicts(bound))
        assertEquals(listOf(ReaderKeymap.NEXT_CHAPTER), ReaderKeymap.conflictsWith(bound, ReaderKeymap.NEXT_PAGE, "N"))
        assertEquals(emptyMap<String, String>(), ReaderKeymap.bind(bound, ReaderKeymap.NEXT_PAGE, "DPAD_RIGHT"))
        assertEquals("DPAD_LEFT", ReaderKeymap.keyName("KEYCODE_DPAD_LEFT"))
    }

    @Test
    fun readlistMergeNewerWins() {
        val media = Media(MediaRef(anilistId = 1), MediaKind.MANGA, "B", emptyMap(), null, null, null, null, emptyList(), 100, null, null, null, null, "anilist")
        val local = listOf(ReadEntry(MediaRef(anilistId = 1), "B", null, ListStatus.WATCHING, progress = 10, updatedAtMs = 5_000_000))
        val older = AniListReadRow(media, "MANGA", ListStatus.PLANNING, 2, null, updatedAtSec = 10, totalChapters = 100)
        assertEquals(10, ReadlistStore.merge(local, listOf(older)).single().progress)
        val newer = older.copy(status = ListStatus.COMPLETED, progress = 100, updatedAtSec = 9_000)
        var changed = 0
        val merged = ReadlistStore.merge(local, listOf(newer)) { changed = it }
        assertEquals(ListStatus.COMPLETED, merged.single().status)
        assertEquals(1, changed)
        val other = older.copy(media = media.copy(ref = MediaRef(anilistId = 2)))
        assertEquals(2, ReadlistStore.merge(local, listOf(other)).size)
        assertTrue(ReadlistStore.isNovel("novel"))
    }

    @Test
    fun pageFileNames() {
        assertEquals("0003.png", PageFiles.fileName(3, "https://x/y/page.png?token=1", null))
        assertEquals("0000.webp", PageFiles.fileName(0, "https://x/y/page", "image/webp"))
        assertEquals("0012.jpg", PageFiles.fileName(12, "https://x/y/page", null))
        assertEquals("a_b", safeFileName("a_b"))
        assertFalse(safeFileName("e/s/1?x").contains('/'))
    }

    @Test
    fun downloadIdsAndRefs() {
        assertEquals("e/s/c1", MangaRepositoryImpl.downloadId(ch("c1", 1.0)))
        assertEquals(MediaRef(malId = 5), MangaRepositoryImpl.refOf("mal:5"))
        assertEquals(MediaRef(anilistId = 30002), MangaRepositoryImpl.refOf("30002"))
        assertEquals("Vol. 2 Ch. 10 · Hi", MangaRepositoryImpl.chapterLabel(ch("x", 10.0, vol = 2).copy(title = "Hi")))
    }
}
