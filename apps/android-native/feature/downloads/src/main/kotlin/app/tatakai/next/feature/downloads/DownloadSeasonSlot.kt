// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.Media

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r; checklist downloads.season-modal,
 * details.download-season-mount): mounted by `WaveBSlots` of Details (o's file, closed) under the episode list. Renders the
 * "Descargar temporada" button (`download_season` of core/ui) that opens an `AdaptiveSheet` with episode range / language / server
 * (`DownloadSettings.preferredServer / preferredAudioLang` as defaults), shows how many are already downloaded
 * (`DownloadRepository.downloadedEpisodes`) and calls `enqueueSeason`. Empty until r implements it (nothing is drawn).
 */
@Composable
fun DownloadSeasonSlot(media: Media, modifier: Modifier = Modifier) {
    // wave B (r): implement as described above.
}

/**
 * Settings › Descargas (`SettingsSection.DOWNLOADS`, mounted by the contract in SettingsScreen; checklist downloads.location / .prefs /
 * .storage-limit / .auto.* / .android.auto-evict): folder (SAF tree picker, `DownloadSettings.directoryTreeUri`), max concurrent, Wi-Fi
 * only, storage limit + usage (`DownloadRepository.storageUsedBytes`), auto-check + profile, auto-evict, preferred server / audio.
 * Same shape as `PlayerSettingsExtras` of wave A. Empty until r implements it.
 */
@Composable
fun DownloadsSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    // wave B (r): rows described above.
}
