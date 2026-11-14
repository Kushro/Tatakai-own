// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.player.MPV_OPEN_ERROR_CODE
import app.tatakai.next.player.MPV_START_TIMEOUT_CODE
import kotlin.test.Test
import kotlin.test.assertEquals

class HlsPrefetchOptionsTest {
    private val pcUrl = "http://192.168.1.43:8099/api/server/v1/media/abc/master.m3u8"

    @Test
    fun theFilteredLocalMasterIsOpenedWithTheNetworkProtocolsAllowed() {
        // BUG-080: FFmpeg's default for a local master is "file,crypto,data" and it refused the master's http playlists.
        val local = "/data/user/0/app.tatakai.next/cache/mpv-hls/master-123.m3u8"
        assertEquals("protocol_whitelist=[file,http,https,tcp,tls,crypto,data]", HlsPrefetch.lavfOptions(local, pcUrl))
        assertEquals("protocol_whitelist=[file,http,https,tcp,tls,crypto,data]", HlsPrefetch.lavfOptions("file://$local", pcUrl))
    }

    @Test
    fun everyOtherLoadKeepsFfmpegsDefaults() {
        assertEquals("", HlsPrefetch.lavfOptions(pcUrl, pcUrl), "the master was not filtered")
        assertEquals("", HlsPrefetch.lavfOptions("https://cdn.example/x.m3u8", pcUrl))
        assertEquals("", HlsPrefetch.lavfOptions("/sdcard/Download/ep.mkv", "/sdcard/Download/ep.mkv"), "a local file played as it is")
    }

    @Test
    fun anErrorBeforeTheFirstFrameOfANetworkStreamIsAnOpenError() {
        assertEquals(MPV_OPEN_ERROR_CODE, mpvErrorCode("ffmpeg: http: Protocol 'http' not on whitelist", started = false, watched = true))
        assertEquals(MPV_START_TIMEOUT_CODE, mpvErrorCode(START_TIMEOUT, started = false, watched = true))
        assertEquals(-1, mpvErrorCode("loading failed", started = false, watched = false), "torrents and local files fail as usual")
        assertEquals(-1, mpvErrorCode("network error", started = true, watched = true), "after playback started")
    }
}
