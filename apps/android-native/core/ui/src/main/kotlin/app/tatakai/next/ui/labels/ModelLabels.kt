// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.labels

import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.torrent.AfterWatch
import app.tatakai.next.model.torrent.TorrentSection
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TrackerStatus
import app.tatakai.next.ui.R

// android wave contract (docs/redesign/12 §3.10): shared labels of model enums. The keys live in :core:ui values/ + values-es/
// (gradle.properties has nonTransitiveRClass=true: reference them as app.tatakai.next.ui.R.string.*). Implemented here (contract).

fun TorrentState.labelRes(): Int = when (this) {
    TorrentState.INITIALIZING -> R.string.torrent_state_initializing
    TorrentState.DOWNLOADING -> R.string.torrent_state_downloading
    TorrentState.SEEDING -> R.string.torrent_state_seeding
    TorrentState.PAUSED -> R.string.torrent_state_paused
    TorrentState.ERROR -> R.string.torrent_state_error
}

fun TrackerStatus.labelRes(): Int = when (this) {
    TrackerStatus.WORKING -> R.string.torrent_tracker_working
    TrackerStatus.UPDATING -> R.string.torrent_tracker_updating
    TrackerStatus.FAILED -> R.string.torrent_tracker_failed
    TrackerStatus.UNKNOWN -> R.string.torrent_tracker_unknown
}

fun TorrentSection.labelRes(): Int = when (this) {
    TorrentSection.OVERVIEW -> R.string.torrent_section_overview
    TorrentSection.FILES -> R.string.torrent_section_files
    TorrentSection.PEERS -> R.string.torrent_section_peers
    TorrentSection.TRACKERS -> R.string.torrent_section_trackers
    TorrentSection.LIBRARY -> R.string.torrent_section_library
    TorrentSection.DOWNLOADS -> R.string.torrent_section_downloads
    TorrentSection.HISTORY -> R.string.torrent_section_history
    TorrentSection.DEBRID -> R.string.torrent_section_debrid
    TorrentSection.SETTINGS -> R.string.torrent_section_settings
}

fun AfterWatch.labelRes(): Int = when (this) {
    AfterWatch.KEEP -> R.string.torrent_after_watch_keep
    AfterWatch.DELETE -> R.string.torrent_after_watch_delete
}

fun SkipKind.labelRes(): Int = when (this) {
    SkipKind.INTRO -> R.string.player_skip_intro
    SkipKind.OUTRO -> R.string.player_skip_outro
    SkipKind.RECAP -> R.string.player_skip_recap
}

fun SourceKind.labelRes(): Int = when (this) {
    SourceKind.WEB -> R.string.src_web
    SourceKind.TORRENT -> R.string.src_torrent
}

// parity wave A (docs/redesign/15 §3.7): list labels shared by Library (q), ListButton (q, mounted by o) and Settings (q). Implemented here (contract).

fun ListStatus.labelRes(): Int = when (this) {
    ListStatus.WATCHING -> R.string.list_status_watching
    ListStatus.PLANNING -> R.string.list_status_planning
    ListStatus.COMPLETED -> R.string.list_status_completed
    ListStatus.PAUSED -> R.string.list_status_paused
    ListStatus.DROPPED -> R.string.list_status_dropped
    ListStatus.REPEATING -> R.string.list_status_repeating
}

fun ListProvider.labelRes(): Int = when (this) {
    ListProvider.ANILIST -> R.string.list_provider_anilist
    ListProvider.MAL -> R.string.list_provider_mal
}

// parity wave B (docs/redesign/16 §3.10): download states (Downloads page of r, Details badges), readlist labels (readers of s, same
// ListStatus enum as the anime list), chapter kinds. Implemented here (contract).

fun DownloadState.labelRes(): Int = when (this) {
    DownloadState.QUEUED -> R.string.download_state_queued
    DownloadState.DOWNLOADING -> R.string.download_state_downloading
    DownloadState.PAUSED -> R.string.download_state_paused
    DownloadState.COMPLETED -> R.string.download_state_completed
    DownloadState.FAILED -> R.string.download_state_failed
    DownloadState.REMOVING -> R.string.download_state_removing
    DownloadState.BROKEN -> R.string.download_state_broken
}

/** The manga / novel wording of a [ListStatus] ("Reading" instead of "Watching"). */
fun ListStatus.readLabelRes(): Int = when (this) {
    ListStatus.WATCHING -> R.string.read_status_watching
    ListStatus.PLANNING -> R.string.read_status_planning
    ListStatus.COMPLETED -> R.string.read_status_completed
    ListStatus.PAUSED -> R.string.read_status_paused
    ListStatus.DROPPED -> R.string.read_status_dropped
    ListStatus.REPEATING -> R.string.read_status_repeating
}

fun ChapterKind.labelRes(): Int = when (this) {
    ChapterKind.MANGA -> R.string.chapter_kind_manga
    ChapterKind.NOVEL -> R.string.chapter_kind_novel
}

/**
 * BUG-072 (UX-13): an AniList genre ("Action", "Sci-Fi", "Slice of Life") as a string resource, with the desktop's words
 * (`tk-ui` i18n `search.genre.*`); null for a genre without a translation (shown as AniList sends it).
 */
fun genreLabelRes(genre: String): Int? = when (genre.trim().lowercase().replace(' ', '_').replace('-', '_')) {
    "action" -> R.string.genre_action
    "adventure" -> R.string.genre_adventure
    "comedy" -> R.string.genre_comedy
    "drama" -> R.string.genre_drama
    "ecchi" -> R.string.genre_ecchi
    "fantasy" -> R.string.genre_fantasy
    "horror" -> R.string.genre_horror
    "mahou_shoujo" -> R.string.genre_mahou_shoujo
    "mecha" -> R.string.genre_mecha
    "music" -> R.string.genre_music
    "mystery" -> R.string.genre_mystery
    "psychological" -> R.string.genre_psychological
    "romance" -> R.string.genre_romance
    "sci_fi" -> R.string.genre_sci_fi
    "slice_of_life" -> R.string.genre_slice_of_life
    "sports" -> R.string.genre_sports
    "supernatural" -> R.string.genre_supernatural
    "thriller" -> R.string.genre_thriller
    "hentai" -> R.string.genre_hentai
    else -> null
}

/** BUG-072: [genreLabelRes] in the UI language, or the genre as AniList sends it. */
@androidx.compose.runtime.Composable
fun genreLabel(genre: String): String = genreLabelRes(genre)?.let { androidx.compose.ui.res.stringResource(it) } ?: genre

/** BUG-098: AniList `MediaSource` ("LIGHT_NOVEL", "OTHER"…) as a string resource; same words as Details (desktop `details.source.*`). */
fun mediaSourceLabelRes(source: String): Int = when (source.trim().uppercase()) {
    "ORIGINAL" -> R.string.media_source_original
    "MANGA" -> R.string.media_source_manga
    "LIGHT_NOVEL" -> R.string.media_source_light_novel
    "NOVEL" -> R.string.media_source_novel
    "WEB_NOVEL" -> R.string.media_source_web_novel
    "VISUAL_NOVEL" -> R.string.media_source_visual_novel
    "VIDEO_GAME", "GAME" -> R.string.media_source_game
    "WEB_MANGA" -> R.string.media_source_web_manga
    "ANIME" -> R.string.media_source_anime
    "DOUJINSHI" -> R.string.media_source_doujinshi
    "LIVE_ACTION" -> R.string.media_source_live_action
    "COMIC" -> R.string.media_source_comic
    "MULTIMEDIA_PROJECT" -> R.string.media_source_multimedia
    "PICTURE_BOOK" -> R.string.media_source_picture_book
    else -> R.string.media_source_other
}

/** BUG-098: AniList `CharacterRole` (MAIN / SUPPORTING / BACKGROUND); null for an unknown value (shown as it comes). */
fun characterRoleLabelRes(role: String): Int? = when (role.trim().uppercase()) {
    "MAIN" -> R.string.character_role_main
    "SUPPORTING" -> R.string.character_role_supporting
    "BACKGROUND" -> R.string.character_role_background
    else -> null
}

/**
 * BUG-098: an AniList staff role of a manga ("Story", "Art", "Story & Art", "Original Story"…). AniList adds details in parentheses
 * ("Story (ch 1-20)"): only the part before them is translated, the rest is kept. null = no translation (shown as it comes).
 */
fun staffRoleLabelRes(role: String): Int? = when (role.substringBefore('(').trim().lowercase()) {
    "story & art", "story and art" -> R.string.staff_role_story_art
    "story" -> R.string.staff_role_story
    "art" -> R.string.staff_role_art
    "original story" -> R.string.staff_role_original_story
    "original creator" -> R.string.staff_role_original_creator
    "character design" -> R.string.staff_role_character_design
    "illustration" -> R.string.staff_role_illustration
    "assistant" -> R.string.staff_role_assistant
    "translator" -> R.string.staff_role_translator
    "lettering" -> R.string.staff_role_lettering
    "editing", "editor" -> R.string.staff_role_editing
    else -> null
}

@androidx.compose.runtime.Composable
fun mediaSourceLabel(source: String): String = androidx.compose.ui.res.stringResource(mediaSourceLabelRes(source))

@androidx.compose.runtime.Composable
fun characterRoleLabel(role: String): String =
    characterRoleLabelRes(role)?.let { androidx.compose.ui.res.stringResource(it) } ?: role.lowercase().replaceFirstChar(Char::uppercase)

@androidx.compose.runtime.Composable
fun staffRoleLabel(role: String): String {
    val res = staffRoleLabelRes(role) ?: return role
    val rest = role.substring(role.indexOf('(').takeIf { it >= 0 } ?: role.length).trim()
    val base = androidx.compose.ui.res.stringResource(res)
    return if (rest.isEmpty()) base else "$base $rest"
}
