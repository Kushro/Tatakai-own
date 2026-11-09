// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadProfile
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.WebInfo
import java.nio.file.Files
import java.time.Instant
import java.time.LocalDate
import javax.crypto.Cipher
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * parity wave B (16 §5 r.7 g, worker r): queue rules, storage limit, integrity check on real files, auto-download profiles, the JSON
 * stores and the HLS AES-128 helper of :core:download. The Media3 / Android side (DownloadManager, SAF) is covered on device (F8).
 */
class DownloadRepositoryImplTest {
    private val ref = MediaRef(anilistId = 21)

    private fun item(ep: Int, state: DownloadState, total: Long? = null, created: Long = ep.toLong(), uri: String? = null) = DownloadItem(
        id = DownloadLogic.itemId(ref, ep), ref = ref, title = "One Piece", episode = ep, extId = "ext", sourceLabel = "Alpha",
        container = Container.HLS, state = state, totalBytes = total, createdAtMs = created, completedAtMs = if (state == DownloadState.COMPLETED) created else null,
        localUri = uri,
    )

    private fun web(label: String, q: Quality, dubs: List<String> = emptyList()) = RankedSource(
        SourceCandidate(extId = "ext", extName = "Ext", kind = SourceKind.WEB, label = label, quality = q, container = "HLS", dubs = dubs, web = WebInfo("https://h/$label.m3u8")),
        score = 1f,
    )

    @Test fun idsFollowTheHistoryShape() {
        assertEquals("21?ep=3", DownloadLogic.itemId(ref, 3))
        assertEquals("mal:5?ep=1", DownloadLogic.itemId(MediaRef(malId = 5), 1))
        assertEquals("mal_5", DownloadLogic.dirName(MediaRef(malId = 5)))
        assertEquals("007.ts", DownloadLogic.fileName(7, Container.HLS))
        assertEquals("012.mp4", DownloadLogic.fileName(12, Container.MP4))
    }

    @Test fun queueSummaryAndHistory() {
        val list = listOf(item(1, DownloadState.DOWNLOADING), item(2, DownloadState.QUEUED), item(3, DownloadState.COMPLETED, total = 10), item(4, DownloadState.FAILED), item(5, DownloadState.PAUSED))
        val s = DownloadLogic.summary(list)
        assertEquals(2, s.active)
        assertEquals(1, s.completed)
        assertEquals(1, s.failed)
        assertEquals(10, s.bytes)
        assertEquals(listOf(5, 4, 3, 2, 1), DownloadLogic.sorted(list).map { it.episode })
        assertEquals(listOf(4, 3), DownloadLogic.history(list, 10).map { it.episode })
        assertTrue(DownloadLogic.isActive(DownloadState.PAUSED))
        assertFalse(DownloadLogic.isActive(DownloadState.COMPLETED))
    }

    @Test fun storageLimit() {
        assertFalse(DownloadLogic.overLimit(Long.MAX_VALUE / 2, 0))
        assertFalse(DownloadLogic.overLimit(DownloadLogic.GIB - 1, 1))
        assertTrue(DownloadLogic.overLimit(DownloadLogic.GIB, 1))
    }

    @Test fun integrityOnRealFiles() {
        val dir = Files.createTempDirectory("dl").toFile()
        val good = java.io.File(dir, "good.ts").apply { writeBytes(ByteArray(100)) }
        val short = java.io.File(dir, "short.ts").apply { writeBytes(ByteArray(10)) }
        val missing = java.io.File(dir, "missing.ts")
        fun check(f: java.io.File, total: Long?) = DownloadLogic.isBroken(item(1, DownloadState.COMPLETED, total), f.exists(), if (f.exists()) f.length() else 0)
        assertFalse(check(good, 100))
        assertTrue(check(short, 100))
        assertTrue(check(missing, null))
        assertFalse(DownloadLogic.isBroken(item(1, DownloadState.DOWNLOADING), false, 0))
        assertEquals(110, DownloadLogic.folderBytes(dir))
    }

    @Test fun profilesCapQualityAndPreferServerAndLanguage() {
        val ranked = listOf(web("Alpha", Quality.P2160), web("Beta", Quality.P1080), web("Gamma", Quality.P720, dubs = listOf("es")), web("Delta", Quality.UNKNOWN))
        assertEquals(listOf("Alpha", "Beta", "Gamma", "Delta"), DownloadLogic.candidatesFor(ranked, DownloadProfile.BEST).map { it.candidate.label })
        assertEquals(listOf("Beta", "Gamma", "Delta"), DownloadLogic.candidatesFor(ranked, DownloadProfile.BALANCED_1080).map { it.candidate.label })
        assertEquals(listOf("Gamma", "Delta"), DownloadLogic.candidatesFor(ranked, DownloadProfile.SAVER_720).map { it.candidate.label })
        assertEquals("Gamma", DownloadLogic.candidatesFor(ranked, DownloadProfile.BEST, preferredLang = "es").first().candidate.label)
        assertEquals("Delta", DownloadLogic.candidatesFor(ranked, DownloadProfile.BEST, preferredServer = "delta").first().candidate.label)
    }

    @Test fun variantPick() {
        val v = listOf(DownloadLogic.Variant(0, 360, 800), DownloadLogic.Variant(1, 1080, 5000), DownloadLogic.Variant(2, 720, 2500), DownloadLogic.Variant(3, 2160, 15000))
        assertEquals(3, DownloadLogic.pickVariant(v, DownloadProfile.BEST)?.index)
        assertEquals(1, DownloadLogic.pickVariant(v, DownloadProfile.BALANCED_1080)?.index)
        assertEquals(2, DownloadLogic.pickVariant(v, DownloadProfile.SAVER_720)?.index)
        assertEquals(0, DownloadLogic.pickVariant(listOf(DownloadLogic.Variant(0, 1080, 1), DownloadLogic.Variant(1, 2160, 2)), DownloadProfile.SAVER_720)?.index)
    }

    @Test fun autoDownloadNewAiredEpisodes() {
        val rule = AutoDownloadRule(ref = ref, title = "t", lastEpisode = 2)
        val eps = listOf(
            Episode(1, null, null, "2026-10-01"), Episode(2, null, null, "2026-10-08"), Episode(3, null, null, "2026-10-15"),
            Episode(4, null, null, "2026-10-22"), Episode(5, null, null, "2026-12-01"),
        )
        assertEquals(listOf(3, 4), DownloadLogic.newEpisodes(rule, eps, have = emptySet(), today = LocalDate.of(2026, 11, 9)))
        assertEquals(listOf(4), DownloadLogic.newEpisodes(rule, eps, have = setOf(3), today = LocalDate.of(2026, 11, 9)))
        val noDates = (1..10).map { Episode(it, null, null, null) }
        assertEquals(listOf(3, 4, 5), DownloadLogic.newEpisodes(rule, noDates, emptySet(), LocalDate.now(), airedCount = 6))
    }

    @Test fun seasonSkipsDoneAndQueued() {
        val existing = mapOf(1 to item(1, DownloadState.COMPLETED), 2 to item(2, DownloadState.QUEUED), 3 to item(3, DownloadState.FAILED))
        assertEquals(listOf(3, 4), DownloadLogic.missingEpisodes(listOf(4, 3, 2, 1, 4), existing))
    }

    @Test fun evictsWatchedEpisodes() {
        fun h(ep: Int, done: Boolean) = HistoryEntry("21", "t", null, "21?ep=$ep", ep, 0, 1400, done, Instant.EPOCH, 21, null)
        val list = listOf(item(1, DownloadState.COMPLETED), item(2, DownloadState.COMPLETED), item(3, DownloadState.DOWNLOADING))
        assertEquals(listOf(1), DownloadLogic.watchedToEvict(list, listOf(h(1, true), h(2, false), h(3, true))).map { it.episode })
    }

    @Test fun localGuessUsesTheReleaseParser() {
        val g = DownloadLogic.guessLocal("[SubsPlease] Sousou no Frieren - 05 (1080p) [ABCD1234].mkv")
        assertEquals(5, g.episode)
        assertTrue(g.title.orEmpty().contains("Frieren"))
        assertTrue(DownloadLogic.isVideo("a.mkv"))
        assertFalse(DownloadLogic.isVideo("a.srt"))
    }

    @Test fun storeRoundTrips() {
        val dir = Files.createTempDirectory("store").toFile()
        val store = DownloadStore(dir)
        assertTrue(store.downloads().isEmpty())
        val rec = StoredDownload(item(1, DownloadState.QUEUED), StreamSource("https://h/a.m3u8", mapOf("Referer" to "https://h"), Container.HLS, "1080p", "Alpha", "ext"), "https://h/a_1080.m3u8")
        store.saveDownloads(listOf(rec))
        store.saveRules(listOf(AutoDownloadRule(ref, "t")))
        store.saveHiddenHistory(listOf("x"))
        val again = DownloadStore(dir)
        assertEquals(listOf(rec), again.downloads())
        assertEquals(1, again.rules().size)
        assertEquals(listOf("x"), again.hiddenHistory())
        java.io.File(dir, DownloadStore.RULES).writeText("{corrupt")
        assertTrue(again.rules().isEmpty())
    }

    @Test fun hlsAes128() {
        val key = ByteArray(16) { it.toByte() }
        val iv = HlsCrypto.iv(null, 7)
        assertEquals(7, iv[15].toInt())
        assertContentEquals(ByteArray(15) + byteArrayOf(0x2a), HlsCrypto.iv("0x2A", 0))
        val plain = "segment bytes".toByteArray()
        val enc = Cipher.getInstance("AES/CBC/PKCS5Padding").apply { init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), IvParameterSpec(iv)) }.doFinal(plain)
        assertContentEquals(plain, HlsCrypto.decrypt(enc, key, iv))
    }
}
