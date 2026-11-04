// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.SkipKind
import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

// Same body as the desktop test of tk-app/src/live/torrent_play.rs (aniskip_bodies_map_to_marks).
class AniSkipParserTest {
    @Test
    fun aniSkipBodiesMapToMarks() {
        val body = """{"found":true,"results":[
            {"interval":{"startTime":1320.5,"endTime":1410.0},"skipType":"ed","skipId":"b","episodeLength":1440.0},
            {"interval":{"startTime":84.0,"endTime":174.0},"skipType":"op","skipId":"a","episodeLength":1440.0},
            {"interval":{"startTime":0.0,"endTime":30.0},"skipType":"recap","skipId":"c","episodeLength":1440.0},
            {"interval":{"startTime":10.0,"endTime":5.0},"skipType":"op","skipId":"bad","episodeLength":1440.0},
            {"interval":{"startTime":10.0,"endTime":20.0},"skipType":"preview","skipId":"d","episodeLength":1440.0}
        ],"message":"Successfully found skip times","statusCode":200}"""
        val marks = AniSkipRepository.parseAniSkip(body)
        assertEquals(listOf(SkipKind.RECAP, SkipKind.INTRO, SkipKind.OUTRO), marks.map { it.kind })
        assertEquals(84.0, marks[1].startSecs)
        assertEquals(174.0, marks[1].endSecs)
        assertTrue(AniSkipRepository.parseAniSkip("""{"found":false,"results":[],"statusCode":404}""").isEmpty())
        assertTrue(AniSkipRepository.parseAniSkip("not json").isEmpty())
        val mixed = AniSkipRepository.parseAniSkip("""{"found":true,"results":[{"interval":{"startTime":1.0,"endTime":2.0},"skipType":"mixed-op"}]}""")
        assertEquals(SkipKind.INTRO, mixed[0].kind)
    }

    @Test
    fun noMalIdMeansNoNetworkAndNoMarks() = runTest {
        // An unroutable base URL: if the repository tried the network the call would fail, but it must not even try.
        val repo = AniSkipRepository(OkHttpClient(), baseUrl = "http://127.0.0.1:9")
        assertTrue(repo.skipMarks(MediaRef(anilistId = 21), 1).isEmpty())
        assertTrue(repo.skipMarks(MediaRef(malId = 0), 1).isEmpty())
    }
}
