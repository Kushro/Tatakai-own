// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.AppSettings

/**
 * Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.1). Owner: worker o. Mounted by Settings › Metadatos
 * (worker q's module) after the cache rows, so o owns these rows AND their strings (`search_settings_*` in :feature:search).
 *
 * Rows: preferred title language (`AppSettings.titleLanguage`: Romaji / English / Native), 18+ content (`AppSettings.showAdult`, with the
 * classic "warn before showing" behaviour handled by the Details adult gate), "Borrar búsquedas recientes" (SearchHistoryStore).
 */
@Composable
fun DiscoverSettingsExtras(
    settings: AppSettings,
    onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("DiscoverSettingsExtras", modifier)
}
