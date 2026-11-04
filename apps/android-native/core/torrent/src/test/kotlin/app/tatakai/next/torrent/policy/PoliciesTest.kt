// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

import app.tatakai.next.model.torrent.TorrentSettings
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PoliciesTest {

    @Test
    fun `after watch removes transient torrents with their files`() {
        assertEquals(
            AfterWatchAction.REMOVE_WITH_FILES,
            afterWatchAction(persist = false, backgroundDownload = false, backgroundEnabled = true, complete = false, seed = false, openStreams = 0),
        )
    }

    @Test
    fun `after watch does nothing while another stream is open`() {
        assertNull(afterWatchAction(persist = false, backgroundDownload = false, backgroundEnabled = false, complete = false, seed = false, openStreams = 1))
    }

    @Test
    fun `after watch keeps background downloads and persisted torrents`() {
        // background download of a transient play: never removed
        assertEquals(
            AfterWatchAction.KEEP_RUNNING,
            afterWatchAction(persist = false, backgroundDownload = true, backgroundEnabled = true, complete = false, seed = false, openStreams = 0),
        )
        // persisted with background off: paused (desktop 10 §4.3)
        assertEquals(
            AfterWatchAction.PAUSE,
            afterWatchAction(persist = true, backgroundDownload = false, backgroundEnabled = false, complete = false, seed = false, openStreams = 0),
        )
        // persisted with background on: keeps going
        assertEquals(
            AfterWatchAction.KEEP_RUNNING,
            afterWatchAction(persist = true, backgroundDownload = false, backgroundEnabled = true, complete = false, seed = false, openStreams = 0),
        )
    }

    @Test
    fun `complete torrents never seed unless asked`() {
        assertEquals(
            AfterWatchAction.PAUSE,
            afterWatchAction(persist = true, backgroundDownload = true, backgroundEnabled = true, complete = true, seed = false, openStreams = 0),
        )
        assertEquals(
            AfterWatchAction.KEEP_RUNNING,
            afterWatchAction(persist = true, backgroundDownload = true, backgroundEnabled = true, complete = true, seed = true, openStreams = 0),
        )
    }

    private val bgOn = TorrentSettings(backgroundDownloads = true, backgroundAck = true)
    private fun facts(
        streaming: Boolean = false,
        playing: Boolean = false,
        userPaused: Boolean = false,
        userResumed: Boolean = false,
        complete: Boolean = false,
        persist: Boolean = true,
        background: Boolean = true,
        seed: Boolean = false,
    ) = RunFacts(streaming, playing, userPaused, userResumed, complete, persist, background, seed)

    @Test
    fun `streams always run, even on metered data with low battery`() {
        val bad = DeviceConditions(connected = true, metered = true, batteryLow = true)
        assertTrue(shouldRun(facts(streaming = true, userPaused = true), bgOn, bad))
        assertTrue(shouldRun(facts(playing = true), TorrentSettings(), bad))
    }

    @Test
    fun `background downloads follow wifiOnly and battery`() {
        val wifi = DeviceConditions(connected = true, metered = false)
        val cell = DeviceConditions(connected = true, metered = true)
        assertTrue(shouldRun(facts(), bgOn, wifi))
        assertFalse(shouldRun(facts(), bgOn, cell))
        assertTrue(shouldRun(facts(), bgOn.copy(wifiOnly = false), cell))
        assertFalse(shouldRun(facts(), bgOn, wifi.copy(batteryLow = true)))
        assertTrue(shouldRun(facts(), bgOn.copy(pauseOnLowBattery = false), wifi.copy(batteryLow = true)))
        assertFalse(shouldRun(facts(), bgOn, wifi.copy(pausedBySystem = true)))
        assertFalse(shouldRun(facts(), bgOn, DeviceConditions(connected = false)))
    }

    @Test
    fun `background off or not acknowledged pauses saved torrents`() {
        val wifi = DeviceConditions()
        assertFalse(shouldRun(facts(), TorrentSettings(backgroundDownloads = true, backgroundAck = false), wifi))
        assertFalse(shouldRun(facts(), TorrentSettings(backgroundDownloads = false, backgroundAck = true), wifi))
        assertFalse(shouldRun(facts(persist = false, background = false), bgOn, wifi))
    }

    @Test
    fun `user pause wins and explicit resume overrides wifiOnly`() {
        val cell = DeviceConditions(connected = true, metered = true)
        assertFalse(shouldRun(facts(userPaused = true), bgOn, DeviceConditions()))
        assertTrue(shouldRun(facts(userResumed = true), TorrentSettings(), cell))
    }

    @Test
    fun `complete torrents run only when seeding is enabled and allowed`() {
        val wifi = DeviceConditions()
        assertFalse(shouldRun(facts(complete = true), bgOn, wifi))
        assertTrue(shouldRun(facts(complete = true, seed = true), bgOn, wifi))
        assertFalse(shouldRun(facts(complete = true, seed = true), bgOn, DeviceConditions(metered = true)))
    }

    @Test
    fun `upload cap on metered networks`() {
        assertEquals(0, uploadLimitBytes(TorrentSettings(), metered = false))
        assertEquals(METERED_UPLOAD_BPS, uploadLimitBytes(TorrentSettings(), metered = true))
        assertEquals(500 * 1024, uploadLimitBytes(TorrentSettings(uploadLimitKib = 500), metered = false))
        assertEquals(METERED_UPLOAD_BPS, uploadLimitBytes(TorrentSettings(uploadLimitKib = 500), metered = true))
        assertEquals(500 * 1024, uploadLimitBytes(TorrentSettings(uploadLimitKib = 500, uploadOnMobileData = true), metered = true))
    }

    @Test
    fun `peer flags keep the qBittorrent order`() {
        assertEquals("D U I P E X H S", peerFlagLetters(true, true, true, true, true, true, true, true))
        assertEquals("", peerFlagLetters(false, false, false, false, false, false, false, false))
        assertEquals("D E H", peerFlagLetters(true, false, false, false, true, false, true, false))
    }

    @Test
    fun `settings sanitized clamps like desktop`() {
        val s = TorrentSettings(
            downloadLimitKib = -5,
            uploadLimitKib = 5_000_000,
            listenPort = 80,
            maxPeers = 0,
            backgroundDownloads = true,
            backgroundAck = false,
        ).sanitized()
        assertEquals(0, s.downloadLimitKib)
        assertEquals(TorrentSettings.MAX_LIMIT_KIB, s.uploadLimitKib)
        assertEquals(0, s.listenPort)
        assertEquals(1, s.maxPeers)
        assertFalse(s.backgroundDownloads)
        assertEquals(6881, TorrentSettings(listenPort = 6881, maxPeers = 9999).sanitized().listenPort)
        assertEquals(TorrentSettings.MAX_PEERS, TorrentSettings(maxPeers = 9999).sanitized().maxPeers)
        // Android defaults (11 §2.4)
        val d = TorrentSettings()
        assertEquals(30, d.maxPeers)
        assertFalse(d.upnp)
        assertTrue(d.wifiOnly)
    }
}
