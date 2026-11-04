// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

/** Port of tk-core/tests/sources_ranking.rs + the score / EMA unit tests of sources.rs, against contracts/fixtures/sources/sources-for.json. */
class SourceRankingTest {
    private val json = Json { ignoreUnknownKeys = true }

    private fun candidates(): List<SourceCandidate> {
        val root = ContractFixtures.element("sources", "sources-for.json").jsonObject
        val list = json.decodeFromJsonElement(ListSerializer(SourceCandidate.serializer()), root["candidates"]!!)
        assertEquals(11, list.size)
        return list
    }

    private fun order(prefs: SourcePrefs, enabled: (String) -> Boolean = { true }): List<String> =
        rank(candidates(), prefs, enabled).map { it.first.label }

    private val defaultOrder =
        listOf("Erai-raws", "Varyg", "SubsPlease", "Alpha", "Beta", "Judas", "Streamwish", "Zulu", "Ok.ru", "Anon-uploader", "Fembed")

    @Test
    fun fixtureMatchesThePrototypeShape() {
        val list = candidates()
        assertEquals(6, list.count { it.kind == SourceKind.WEB })
        for (c in list) {
            when (c.kind) {
                SourceKind.WEB -> assertTrue(c.label, c.web != null && c.torrent == null)
                SourceKind.TORRENT -> assertTrue(c.label, c.torrent != null && c.web == null)
            }
        }
        assertEquals(Quality.P480, list[4].quality)
        assertEquals("HEVC 10-bit", list[9].container)
    }

    @Test
    fun defaultPrefsOrderAndScores() {
        val prefs = SourcePrefs()
        assertEquals(defaultOrder, order(prefs))
        val ranked = rank(candidates(), prefs) { true }
        fun by(label: String) = ranked.first { it.first.label == label }.second
        for ((label, want) in listOf(
            "Erai-raws" to 91.6f, "Varyg" to 84.56f, "SubsPlease" to 83.5f, "Alpha" to 80.7f, "Judas" to 69.94f,
            "Streamwish" to 69.9f, "Anon-uploader" to 21.62f, "Fembed" to 8.5f,
        )) {
            assertTrue("$label: ${by(label)} != $want", abs(by(label) - want) < 1e-3f)
        }
    }

    @Test
    fun manualModeDoesNotChangeTheRanking() {
        assertEquals(defaultOrder, order(SourcePrefs(mode = SourceMode.MANUAL)))
    }

    @Test
    fun preferTorrent() {
        assertEquals(
            listOf("Erai-raws", "Varyg", "SubsPlease", "Judas", "Alpha", "Beta", "Streamwish", "Zulu", "Anon-uploader", "Ok.ru", "Fembed"),
            order(SourcePrefs(prefer = SourceKind.TORRENT)),
        )
    }

    @Test
    fun preferWeb() {
        assertEquals(
            listOf("Alpha", "Beta", "Streamwish", "Zulu", "Ok.ru", "Erai-raws", "Varyg", "SubsPlease", "Judas", "Fembed", "Anon-uploader"),
            order(SourcePrefs(prefer = SourceKind.WEB)),
        )
    }

    @Test
    fun disabledExtensionsSinkToTheBottom() {
        val prefs = SourcePrefs()
        assertEquals(
            listOf("Alpha", "Beta", "Streamwish", "Zulu", "Ok.ru", "Fembed", "Erai-raws", "Varyg", "SubsPlease", "Judas", "Anon-uploader"),
            order(prefs) { it != "io.github.nyaa-own" },
        )
        assertEquals(
            listOf("Erai-raws", "Varyg", "SubsPlease", "Judas", "Streamwish", "Ok.ru", "Anon-uploader", "Fembed", "Alpha", "Beta", "Zulu"),
            order(prefs) { it != "io.github.toko" },
        )
        val alpha = candidates().first { it.label == "Alpha" }
        assertTrue(abs(score(alpha, prefs, false) - (80.7f - 1000f)) < 1e-2f)
    }

    private fun web(quality: Quality = Quality.P1080, latency: Int? = null) = SourceCandidate(
        extId = "io.github.toko",
        extName = "Toko",
        kind = SourceKind.WEB,
        label = "Alpha",
        quality = quality,
        container = "HLS",
        subs = listOf("es", "en"),
        web = WebInfo(url = "https://cdn.example/ep.m3u8", latencyMs = latency),
    )

    @Test
    fun unmeasuredLatencyIsNotPenalizedAndTiesAreStable() {
        val prefs = SourcePrefs()
        val a = web()
        // 0.8*50 + 18 + 10 (es sub) = 68, no dub, no latency
        assertTrue(abs(score(a, prefs, true) - 68f) < 1e-4f)
        val slow = web(latency = 5000)
        assertTrue(abs(score(slow, prefs, true) - 58f) < 1e-4f) // capped at 1000 ms
        val order = rank(listOf(slow, slow.copy(label = "Beta"), slow.copy(label = "Gamma")), prefs) { true }.map { it.first.label }
        assertEquals(listOf("Alpha", "Beta", "Gamma"), order)
    }

    @Test
    fun seadexBestTorrentsGetASmallBonus() {
        val prefs = SourcePrefs()
        val t = web().copy(kind = SourceKind.TORRENT, web = null, torrent = TorrentInfo(infoHash = "ab".repeat(20), seeders = 100, leechers = 1))
        val plain = score(t, prefs, true)
        val best = score(t.copy(tags = t.tags.copy(seadexBest = true)), prefs, true)
        assertTrue(abs(best - plain - SEADEX_BEST_BONUS) < 1e-4f)
        val w = web()
        assertTrue(abs(score(w.copy(tags = w.tags.copy(seadexBest = true)), prefs, true) - score(w, prefs, true)) < 1e-4f)
    }

    @Test
    fun emaAndClamp() {
        assertTrue(abs(updateReliability(INITIAL_RELIABILITY, true) - 0.84f) < 1e-6f)
        assertTrue(abs(updateReliability(INITIAL_RELIABILITY, false) - 0.64f) < 1e-6f)
        assertEquals(1f, updateReliability(1f, true))
        assertEquals(0f, updateReliability(0f, false))
        assertTrue(abs(updateReliability(7f, false) - 0.8f) < 1e-6f)
        assertTrue(abs(updateReliability(Float.NaN, true) - 0.84f) < 1e-6f)
    }

    @Test
    fun langCodesNormalize() {
        assertEquals(listOf("es", "en"), langCodes(listOf("es-419", "und", "EN", "", "es_ES")))
    }
}
