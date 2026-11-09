// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// Ported from the desktop tests of tk-ui/src/player_state.rs (FlowState).
class SourceFlowTest {
    private fun src(ext: String, label: String, kind: SourceKind, enabled: Boolean = true, picked: Boolean = false) = RankedSource(
        candidate = SourceCandidate(
            extId = ext.lowercase(),
            extName = ext,
            kind = kind,
            label = label,
            quality = Quality.P1080,
            web = if (kind == SourceKind.WEB) WebInfo("https://cdn.example/$label.m3u8") else null,
            torrent = if (kind == SourceKind.TORRENT) TorrentInfo("a".repeat(40), seeders = 50) else null,
        ),
        score = 0f,
        extEnabled = enabled,
        picked = picked,
    )

    private fun list() = listOf(
        src("Nyaa", "SubsPlease", SourceKind.TORRENT),
        src("Toko", "Alpha", SourceKind.WEB),
        src("Off", "Dead", SourceKind.WEB, enabled = false),
        src("Toko", "Beta", SourceKind.WEB),
    )

    @Test
    fun autoStartsWithTheBestPlayableSourceTorrentsIncluded() {
        val f = SourceFlow(list(), SourceMode.AUTO)
        assertEquals(FlowPhase.RESOLVING, f.phase)
        assertEquals("nyaa|torrent|SubsPlease", f.toResolve()?.key)
        assertEquals(1 to 4, f.position())
    }

    @Test
    fun manualWaitsAndResolvesNothingUntilAPick() {
        val f = SourceFlow(list(), SourceMode.MANUAL)
        assertEquals(FlowPhase.MANUAL, f.phase)
        assertNull(f.toResolve(), "Manual must not start network work before the user picks")
        assertTrue(f.pick("toko|web|Beta"))
        assertEquals(FlowPhase.RESOLVING, f.phase)
        assertEquals("toko|web|Beta", f.toResolve()?.key)
        assertTrue(f.sources.single { it.picked }.key == "toko|web|Beta")
    }

    @Test
    fun aPickWinsInAnyMode() {
        val sources = list().mapIndexed { i, s -> if (i == 3) s.copy(picked = true) else s }
        assertEquals("toko|web|Beta", SourceFlow(sources, SourceMode.MANUAL).toResolve()?.key)
        assertEquals("toko|web|Beta", SourceFlow(sources, SourceMode.AUTO).toResolve()?.key)
    }

    @Test
    fun autoFailsOverInRankedOrderSkippingDisabledExtensions() {
        val f = SourceFlow(list(), SourceMode.AUTO)
        val a1 = f.attempt
        assertEquals("nyaa|torrent|SubsPlease", f.fail(a1, FailKind.NO_PEERS, "no peers"))
        assertEquals("toko|web|Alpha", f.toResolve()?.key)
        assertTrue(f.lastFailedTorrent())
        assertEquals("toko|web|Alpha", f.fail(f.attempt, FailKind.PLAYBACK, "403"))
        assertEquals("toko|web|Beta", f.toResolve()?.key, "the disabled extension is never tried")
        f.fail(f.attempt, FailKind.PLAYBACK, "500")
        assertEquals(FlowPhase.ERROR, f.phase)
        assertEquals(3, f.failed.size)
        assertEquals("Nyaa · SubsPlease", f.failed.first().name)
    }

    @Test
    fun staleAttemptsAreIgnored() {
        val f = SourceFlow(list(), SourceMode.AUTO)
        val old = f.attempt
        f.fail(old, FailKind.PLAYBACK, "x")
        assertNull(f.fail(old, FailKind.PLAYBACK, "late"), "an answer for an older attempt changes nothing")
        assertEquals("toko|web|Alpha", f.toResolve()?.key)
    }

    @Test
    fun manualStopsOnFailureSoTheUserPicksAgain() {
        val f = SourceFlow(list(), SourceMode.MANUAL)
        f.pick("toko|web|Alpha")
        assertEquals("toko|web|Alpha", f.fail(f.attempt, FailKind.PLAYBACK, "403"))
        assertEquals(FlowPhase.ERROR, f.phase)
        assertNull(f.toResolve())
    }

    @Test
    fun startIsReportedOncePerAttempt() {
        val f = SourceFlow(list(), SourceMode.AUTO)
        assertNull(f.started(), "not before the stream was handed to the engine")
        f.resolved(f.attempt)
        assertEquals(FlowPhase.BUFFERING, f.phase)
        assertEquals("nyaa|torrent|SubsPlease", f.started())
        assertNull(f.started())
        f.stalled(true)
        assertEquals(FlowPhase.BUFFERING, f.phase)
        f.stalled(false)
        assertEquals(FlowPhase.PLAYING, f.phase)
    }

    @Test
    fun retryForgetsFailuresAndBestWebIsOffered() {
        val f = SourceFlow(list(), SourceMode.AUTO)
        f.fail(f.attempt, FailKind.NO_PEERS, "")
        assertEquals("toko|web|Alpha", f.bestWeb()?.key)
        f.fail(f.attempt, FailKind.PLAYBACK, "")
        assertEquals("toko|web|Beta", f.bestWeb()?.key, "a web source that has not failed yet comes first")
        f.retry()
        assertTrue(f.failed.isEmpty())
        assertEquals("nyaa|torrent|SubsPlease", f.toResolve()?.key)
    }

    @Test
    fun emptyListIsAnError() {
        val f = SourceFlow(emptyList(), SourceMode.AUTO)
        assertEquals(FlowPhase.ERROR, f.phase)
        assertNull(f.toResolve())
        assertFalse(f.lastFailedTorrent())
    }

    @Test
    fun applyPickPrefersTheRequestedKeyThenTheStoredPick() {
        val base = list()
        assertEquals("toko|web|Beta", applyPick(base, "toko|web|Beta", "toko|web|Alpha").single { it.picked }.key)
        assertEquals("toko|web|Alpha", applyPick(base, "unknown|web|x", "toko|web|Alpha").single { it.picked }.key)
        assertTrue(applyPick(base, null, null).none { it.picked })
        assertEquals(base.map { it.key }, applyPick(base, "toko|web|Beta", null).map { it.key }, "never reorders")
    }

    // parity wave A (15 §3.2, bug sources.remembered-vs-prefs): audit case, JA audio + ES subs preferred, remembered RU torrent pack.
    @Test
    fun rememberedIncompatibleSourceIsIgnoredAndTheRankingPlays() {
        val ru = RankedSource(
            candidate = SourceCandidate(
                extId = "nyaa", extName = "Nyaa", kind = SourceKind.TORRENT, label = "RU pack", quality = Quality.P1080,
                subs = emptyList(), dubs = listOf("ru"), torrent = TorrentInfo("b".repeat(40), seeders = 5),
            ),
            score = 0.1f,
        )
        val anizone = RankedSource(
            candidate = SourceCandidate(
                extId = "anizone", extName = "AniZone", kind = SourceKind.WEB, label = "Main", quality = Quality.P1080,
                subs = listOf("es-419"), dubs = listOf("ja"), web = WebInfo("https://cdn.example/a.m3u8"),
            ),
            score = 0.9f,
            recommended = true,
        )
        val prefs = app.tatakai.next.model.SourcePrefs(subLang = "es", dubLang = "ja", minQuality = Quality.P720)
        val list = listOf(anizone, ru)
        val res = resolvePick(list, requestedKey = null, storedPick = ru.key, prefs = prefs)
        assertTrue(res.storedDiscarded)
        assertEquals(PickMismatch.LANGUAGE, res.mismatch)
        assertTrue(res.sources.none { it.picked })
        assertEquals(list.map { it.key }, res.sources.map { it.key }, "the remembered one stays as an option")
        val flow = SourceFlow(res.sources, SourceMode.AUTO)
        assertEquals(anizone.key, flow.toResolve()?.key)

        // A compatible remembered pick still wins; this session's pick always wins.
        val ok = resolvePick(list, null, anizone.key, prefs)
        assertFalse(ok.storedDiscarded)
        assertEquals(anizone.key, ok.sources.single { it.picked }.key)
        assertEquals(ru.key, resolvePick(list, ru.key, anizone.key, prefs).sources.single { it.picked }.key)
        // Disabled extension or too low quality also drop the remembered pick.
        assertEquals(PickMismatch.EXTENSION_OFF, pickMismatches(anizone.candidate, false, prefs).first())
        assertTrue(PickMismatch.QUALITY in pickMismatches(anizone.candidate.copy(quality = Quality.P480), true, prefs))
    }
}
