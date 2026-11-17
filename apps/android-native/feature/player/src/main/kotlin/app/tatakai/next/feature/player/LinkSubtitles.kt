// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkSubtitleTrack
import app.tatakai.next.model.SubtitleTrack
import app.tatakai.next.player.LanguageCodes
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.SubtitleOption
import java.util.Locale

// Tatakai Next 0.4.0: the source's subtitles while the PC processes (Link "PC" on Media3, "Mixto" on libmpv). The PC's session HLS
// carries video + one audio stream only; tk-link lists the source's subtitle tracks in the session status and serves each one
// (`subtitles/{n}.ass` for ASS / SSA, styles kept, and `.vtt` for any text track), with the source's own timestamps: the session
// HLS uses the same timeline whatever its start, so the files are added as they are. The ideas follow Enigmata's
// `TempleSubtitleLoader` (AGPL-3.0-or-later, same author): only the listed text tracks, named with the file's language and title.
// Pure, no Android.

/** The subtitle the player showed before the PC took over (or before a reload of the PC's stream). */
sealed interface SubtitleChoice {
    /** Tracks were listed and none was on: stay off. */
    data object Off : SubtitleChoice

    /**
     * The selected track: [language] / [label] as the engine lists it, [url] when it was a side-loaded file (a web source's own
     * subtitle, which the PC playback side-loads too), [ordinal] = its position among the stream's own (embedded) tracks, which
     * is the PC's `index` for the same file.
     */
    data class Track(val language: String?, val label: String?, val url: String? = null, val ordinal: Int? = null) : SubtitleChoice
}

/** What the engine shows now ([options] of a load of a source with [external] side-loaded files); null = no track listed yet. */
fun subtitleChoiceOf(options: List<SubtitleOption>, external: List<SubtitleTrack>): SubtitleChoice? {
    if (options.isEmpty()) return null
    val selected = options.firstOrNull { it.selected } ?: return SubtitleChoice.Off
    // mpv does not say which of its tracks are side-loaded (Media3 does, `external`): they are the last ones, after the stream's own
    val tail = if (options.all { it.external == null }) external.size.coerceAtMost(options.size) else 0
    val position = options.indexOfFirst { it.groupIndex == selected.groupIndex }
    val url = selected.external?.let { external.getOrNull(it)?.url }
        ?: (position - (options.size - tail)).takeIf { tail > 0 && it >= 0 }?.let { external.getOrNull(it)?.url }
    val embedded = options.filter { it.external == null }.dropLast(tail)
    val ordinal = embedded.indexOfFirst { it.groupIndex == selected.groupIndex }.takeIf { it >= 0 && url == null }
    return SubtitleChoice.Track(selected.language, selected.label, url, ordinal)
}

/** One PC subtitle track as the player side-loads it. */
data class PcSubtitle(val track: LinkSubtitleTrack, val subtitle: SubtitleTrack)

/**
 * The PC's text tracks for the engine: libmpv gets `.ass` when the source track is ASS / SSA (libass draws its styles), else
 * `.vtt`; Media3 always `.vtt` (its SSA support drops the styling anyway and the screen draws the cues with the user's style).
 * [url] gives the file URL (loopback proxy for libmpv, the PC's own for Media3), null = skip it. Image tracks are left out.
 */
fun pcSubtitles(tracks: List<LinkSubtitleTrack>, mpv: Boolean, url: (index: Int, format: String) -> String?): List<PcSubtitle> =
    tracks.filter { it.text }.mapNotNull { t ->
        val format = when {
            mpv && t.hasAss -> LinkSubtitleTrack.FORMAT_ASS
            t.hasVtt -> LinkSubtitleTrack.FORMAT_VTT
            else -> return@mapNotNull null
        }
        val u = url(t.index, format) ?: return@mapNotNull null
        PcSubtitle(t, SubtitleTrack(url = u, lang = t.language.orEmpty(), label = pcSubtitleLabel(t), format = format))
    }

/** The label of a PC track: the file's title, else its language's name, else its number. */
fun pcSubtitleLabel(t: LinkSubtitleTrack): String {
    t.title?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
    val code = LanguageCodes.normalize(t.language)
    val name = code?.let { Locale.forLanguageTag(it).getDisplayLanguage(Locale.getDefault()) }?.takeIf { it.isNotBlank() && it != code }
    return name?.replaceFirstChar { it.titlecase(Locale.getDefault()) } ?: t.language?.takeIf { it.isNotBlank() } ?: "#${t.index + 1}"
}

/**
 * The entry of [subtitles] (the web source's files first, then the PC's: [pc] maps an index of [subtitles] to its PC track) the
 * player selects on a PC load: the one matching [choice] (same file, else same title and language, else same language, the same
 * position among the source's tracks first), else one in [preferredLanguage] (Settings), else the source's default track. Returns
 * [PlayableSource.SUBTITLES_OFF] when the phone had them off, null when nothing applies (the engine decides). Forced (signs-only)
 * tracks are only picked when nothing else matches.
 */
fun chooseSubtitle(
    subtitles: List<SubtitleTrack>,
    pc: Map<Int, LinkSubtitleTrack>,
    choice: SubtitleChoice?,
    preferredLanguage: String?,
): Int? {
    if (subtitles.isEmpty()) return null
    if (choice == SubtitleChoice.Off) return PlayableSource.SUBTITLES_OFF
    val indices = subtitles.indices.toList()
    fun forced(i: Int) = pc[i]?.forced == true
    fun best(candidates: List<Int>, ordinal: Int? = null): Int? =
        candidates.sortedWith(compareBy({ ordinal == null || pc[it]?.index != ordinal }, { forced(it) })).firstOrNull()
    if (choice is SubtitleChoice.Track) {
        choice.url?.let { url -> indices.firstOrNull { subtitles[it].url == url }?.let { return it } }
        val language = LanguageCodes.normalize(choice.language) ?: LanguageCodes.normalize(choice.label)
        val label = choice.label?.trim()?.lowercase(Locale.ROOT)?.takeIf { it.isNotEmpty() }
        fun sameLanguage(i: Int) = language != null && LanguageCodes.matches(subtitles[i].lang, subtitles[i].label, language)
        if (label != null) {
            best(indices.filter { subtitles[it].label.trim().lowercase(Locale.ROOT) == label && (language == null || sameLanguage(it)) }, choice.ordinal)
                ?.let { return it }
        }
        best(indices.filter { sameLanguage(it) }, choice.ordinal)?.let { return it }
    }
    if (LanguageCodes.normalize(preferredLanguage) != null) {
        val inLanguage = indices.filter { LanguageCodes.matches(subtitles[it].lang, subtitles[it].label, preferredLanguage) }
        best(inLanguage.sortedBy { pc[it]?.default != true })?.let { return it }
    }
    return indices.firstOrNull { pc[it]?.default == true && !forced(it) }
}
