// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class HlsMasterFilterTest {
    private val langs = listOf("ja", "en", "ar", "de", "es-419", "es", "tl", "fr", "hi", "it", "pl", "pt-BR", "ta", "te", "th")

    /** Shape of anizone's master (BUG-059): 15 audio renditions in one group + 3 video variants, relative URIs, a session key. */
    private val anizone = buildString {
        append("#EXTM3U\n#EXT-X-VERSION:6\n#EXT-X-INDEPENDENT-SEGMENTS\n")
        append("#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"keys/p8VRfvee.key\"\n")
        langs.forEachIndexed { i, l ->
            val def = if (l == "ja") "YES" else "NO"
            append("#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",LANGUAGE=\"$l\",NAME=\"$l\",DEFAULT=$def,AUTOSELECT=YES,URI=\"audio/${i}_$l/playlist.m3u8\"\n")
        }
        append("#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.64001e,mp4a.40.2\",AUDIO=\"aud\"\nvideo/360/playlist.m3u8\n")
        append("#EXT-X-STREAM-INF:BANDWIDTH=2800000,RESOLUTION=1280x720,CODECS=\"avc1.64001f,mp4a.40.2\",AUDIO=\"aud\"\nvideo/720/playlist.m3u8\n")
        append("#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,CODECS=\"avc1.640028,mp4a.40.2\",AUDIO=\"aud\"\nvideo/1080/playlist.m3u8\n")
    }
    private val base = "https://suzaku.xin-cdn.xyz/3e05857f/master.m3u8"

    private fun mediaLines(s: String) = s.lines().filter { it.startsWith("#EXT-X-MEDIA") }
    private fun uris(s: String) = s.lines().filter { it.isNotBlank() && !it.startsWith("#") }

    @Test
    fun anEighteenTrackMasterKeepsOneVariantAndThePreferredAudio() {
        val out = HlsMasterFilter.filter(anizone, base, audioLang = "ja", subLang = "es")!!
        val media = mediaLines(out)
        assertEquals(1, media.size)
        assertTrue("LANGUAGE=\"ja\"" in media.single())
        assertTrue("URI=\"https://suzaku.xin-cdn.xyz/3e05857f/audio/0_ja/playlist.m3u8\"" in media.single(), "absolute URI")
        assertEquals(listOf("https://suzaku.xin-cdn.xyz/3e05857f/video/1080/playlist.m3u8"), uris(out), "the highest variant only")
        assertTrue("#EXT-X-SESSION-KEY:METHOD=AES-128,URI=\"https://suzaku.xin-cdn.xyz/3e05857f/keys/p8VRfvee.key\"" in out)
        assertTrue(out.startsWith("#EXTM3U\n"))
    }

    @Test
    fun withoutAMatchTheDefaultRenditionStays() {
        val out = HlsMasterFilter.filter(anizone, base, audioLang = "ko", subLang = null)!!
        assertTrue("LANGUAGE=\"ja\"" in mediaLines(out).single())
    }

    @Test
    fun regionalVariantsMatchThePreferredLanguage() {
        // "es" keeps both es-419 and es: the player still picks with alang.
        val out = HlsMasterFilter.filter(anizone, base, audioLang = "es", subLang = null)!!
        assertEquals(2, mediaLines(out).size)
    }

    @Test
    fun aMediaPlaylistOrAMasterWithNothingToDropIsLeftAlone() {
        assertNull(HlsMasterFilter.filter("#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nseg0.ts\n#EXT-X-ENDLIST\n", base, "ja", "es"))
        assertNull(HlsMasterFilter.filter("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nonly.m3u8\n", base, "ja", "es"))
        assertNull(HlsMasterFilter.filter("<html>", base, "ja", "es"))
    }
}
