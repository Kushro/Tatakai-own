// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkSubtitleTrack
import app.tatakai.next.model.SubtitleTrack
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.SubtitleOption
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.0: the source's subtitles on the PC (Link "PC" / "Mixto"): which files the player side-loads and which one it shows. */
class LinkSubtitlesTest {
    // A ToonsHub-like release as tk-link lists it
    private val english = LinkSubtitleTrack(0, "eng", "English", "ass", default = true, formats = listOf("ass", "vtt"))
    private val latino = LinkSubtitleTrack(1, "spa", "Spanish(Latin America)", "ass", formats = listOf("ass", "vtt"))
    private val spain = LinkSubtitleTrack(2, "spa", "Spanish", "ass", formats = listOf("ass", "vtt"))
    private val signs = LinkSubtitleTrack(3, "eng", "Signs", "subrip", forced = true, formats = listOf("vtt"))
    private val pgs = LinkSubtitleTrack(4, "por", null, "hdmv_pgs_subtitle", text = false)
    private val listed = listOf(english, latino, spain, signs, pgs)

    private val url = { i: Int, f: String -> "http://127.0.0.1:9/t/v1/sessions/s/subtitles/$i.$f" }

    /** The PC's tracks as the player side-loads them after [web], with the map [chooseSubtitle] takes. */
    private fun candidates(web: List<SubtitleTrack> = emptyList(), mpv: Boolean = false): Pair<List<SubtitleTrack>, Map<Int, LinkSubtitleTrack>> {
        val pc = pcSubtitles(listed, mpv, url)
        return (web + pc.map { it.subtitle }) to pc.withIndex().associate { (i, p) -> web.size + i to p.track }
    }

    @Test
    fun libmpvGetsTheStyledAssAndMedia3TheWebVtt() {
        val mpv = pcSubtitles(listed, mpv = true, url)
        assertEquals(listOf(0, 1, 2, 3), mpv.map { it.track.index }, "the image track is left out")
        assertEquals(listOf("ass", "ass", "ass", "vtt"), mpv.map { it.subtitle.format })
        assertEquals(url(1, "ass"), mpv[1].subtitle.url)
        assertEquals("Spanish(Latin America)" to "spa", mpv[1].subtitle.label to mpv[1].subtitle.lang)
        val media3 = pcSubtitles(listed, mpv = false, url)
        assertEquals(listOf("vtt", "vtt", "vtt", "vtt"), media3.map { it.subtitle.format })
        // a file the phone cannot address (unpaired, unknown session) is skipped
        assertEquals(emptyList(), pcSubtitles(listed, mpv = false) { _, _ -> null })
    }

    @Test
    fun untitledTracksAreNamedByLanguageOrNumber() {
        assertEquals("Signs", pcSubtitleLabel(signs))
        assertEquals("#6", pcSubtitleLabel(LinkSubtitleTrack(5, codec = "subrip", formats = listOf("vtt"))))
        assertEquals("xx-test", pcSubtitleLabel(LinkSubtitleTrack(5, "xx-test", formats = listOf("vtt"))))
        assertTrue(pcSubtitleLabel(LinkSubtitleTrack(5, "spa", formats = listOf("vtt"))).isNotBlank())
    }

    @Test
    fun theTrackThePhoneShowedIsSelectedOnThePc() {
        val (subs, pc) = candidates()
        // Media3 on the phone played the torrent MKV: its own tracks, the Latin American one selected
        val phone = listOf(
            SubtitleOption(2, "English", "en", selected = false, ass = true),
            SubtitleOption(3, "Spanish(Latin America)", "es", selected = true, ass = true),
            SubtitleOption(4, "Spanish", "es", selected = false, ass = true),
        )
        val choice = subtitleChoiceOf(phone, emptyList())
        assertEquals(SubtitleChoice.Track("es", "Spanish(Latin America)", url = null, ordinal = 1), choice)
        assertEquals(1, chooseSubtitle(subs, pc, choice, preferredLanguage = "en"))
        // libmpv on the phone names the languages as the file does (`spa`); the title tells the two Spanish tracks apart
        assertEquals(2, chooseSubtitle(subs, pc, SubtitleChoice.Track("spa", "Spanish", ordinal = 2), "en"))
        // titles that differ (another release): the same language at the same position of the source wins
        assertEquals(2, chooseSubtitle(subs, pc, SubtitleChoice.Track("es", "Castellano", ordinal = 2), null))
        assertEquals(1, chooseSubtitle(subs, pc, SubtitleChoice.Track("es", "Español", ordinal = null), null))
        // off on the phone, off on the PC
        assertEquals(PlayableSource.SUBTITLES_OFF, chooseSubtitle(subs, pc, SubtitleChoice.Off, "es"))
        assertEquals(SubtitleChoice.Off, subtitleChoiceOf(phone.map { it.copy(selected = false) }, emptyList()))
        assertNull(subtitleChoiceOf(emptyList(), emptyList()), "no track listed yet: nothing to remember")
    }

    @Test
    fun aWebSourcesOwnFileIsTheSameFileOnThePc() {
        val web = listOf(
            SubtitleTrack("https://cdn.example/es.vtt", "es", "Español", "vtt"),
            SubtitleTrack("https://cdn.example/en.vtt", "en", "English", "vtt"),
        )
        // Media3 marks side-loaded tracks (`external`)
        val media3 = listOf(SubtitleOption(5, "Español", "es", selected = false, external = 0), SubtitleOption(6, "English", "en", selected = true, external = 1))
        val choice = subtitleChoiceOf(media3, web)
        assertEquals(SubtitleChoice.Track("en", "English", url = "https://cdn.example/en.vtt"), choice)
        val (subs, pc) = candidates(web)
        assertEquals(1, chooseSubtitle(subs, pc, choice, "es"))
        // libmpv lists the side-loaded files after the stream's own: the last ones are the source's files
        val mpv = listOf(SubtitleOption(1, "Español", "es", selected = true), SubtitleOption(2, "English", "en", selected = false))
        assertEquals("https://cdn.example/es.vtt", (subtitleChoiceOf(mpv, web) as SubtitleChoice.Track).url)
    }

    @Test
    fun withoutAChoiceTheSettingsLanguageThenTheSourceDefault() {
        val (subs, pc) = candidates()
        // a session started from "Enviar al dispositivo": the preferred language, a full (not forced) track, the default first
        assertEquals(1, chooseSubtitle(subs, pc, null, "es"))
        assertEquals(0, chooseSubtitle(subs, pc, null, "en"), "English, not the forced signs")
        assertEquals(0, chooseSubtitle(subs, pc, null, null), "the source's default track")
        assertEquals(0, chooseSubtitle(subs, pc, SubtitleChoice.Track("fr", "Français"), "fr"), "nothing in French: the default")
        val noDefault = pcSubtitles(listOf(latino, signs), mpv = false, url).let { p -> p.map { it.subtitle } to p.withIndex().associate { (i, x) -> i to x.track } }
        assertNull(chooseSubtitle(noDefault.first, noDefault.second, null, "de"), "nothing applies: the engine decides")
        assertEquals(1, chooseSubtitle(noDefault.first, noDefault.second, null, "en"), "only the forced one is in English")
        assertNull(chooseSubtitle(emptyList(), emptyMap(), SubtitleChoice.Off, "es"))
    }
}
