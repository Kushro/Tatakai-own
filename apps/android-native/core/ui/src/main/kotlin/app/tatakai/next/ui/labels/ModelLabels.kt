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
