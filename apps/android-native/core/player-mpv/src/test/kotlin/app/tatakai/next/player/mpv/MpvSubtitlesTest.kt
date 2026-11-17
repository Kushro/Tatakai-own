// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.model.SubtitleTrack
import app.tatakai.next.player.PlayableSource
import kotlin.test.Test
import kotlin.test.assertEquals

/** 0.4.0: external subtitles reach `sub-add` with their title, language and the track to select (Link "Mixto"). */
class MpvSubtitlesTest {
    @Test
    fun titleLanguageAndTheSelectedTrack() {
        val tracks = listOf(
            SubtitleTrack("http://127.0.0.1:9/t/v1/sessions/s/subtitles/0.ass", "eng", "English", "ass"),
            SubtitleTrack("http://127.0.0.1:9/t/v1/sessions/s/subtitles/1.ass", "spa", "Spanish(Latin America)", "ass"),
            SubtitleTrack("https://cdn.example/x.vtt", "", "", "vtt"),
            SubtitleTrack("https://cdn.example/y.vtt", "es 419;x", "Latino", "vtt"),
        )
        val subs = mpvSubtitles(tracks, select = 1)
        assertEquals(MpvSubtitle(tracks[0].url, "English", "eng", select = false), subs[0])
        assertEquals(MpvSubtitle(tracks[1].url, "Spanish(Latin America)", "spa", select = true), subs[1])
        assertEquals(MpvSubtitle(tracks[2].url, null, null), subs[2], "nothing known: mpv names it")
        assertEquals(null, subs[3].lang, "not a language tag: not handed to mpv")
        assertEquals(listOf(false, false, false, false), mpvSubtitles(tracks, PlayableSource.SUBTITLES_OFF).map { it.select })
        assertEquals(listOf(false, false, false, false), mpvSubtitles(tracks, null).map { it.select })
    }
}
