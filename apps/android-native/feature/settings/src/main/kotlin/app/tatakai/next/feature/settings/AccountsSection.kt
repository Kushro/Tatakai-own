// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable

/**
 * Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Owner: worker q.
 *
 * Settings › Listas y cuentas (`SettingsSection.ACCOUNTS`, classic Integrations panel): one card per provider (AniList, MAL) with
 * "Vincular" (Custom Tab on `ListsRepository.beginLink`), the linked username / avatar, "Desvincular"; pending-changes counter and
 * "Sincronizar listas ahora" (`syncNow`); conflicts list with Keep local / Keep remote; auto-progress switch (`AppSettings.autoListProgress`);
 * Import / Export (SAF `CreateDocument` / `OpenDocument` of `application/json`). Reads `ListsRepository` through [SettingsViewModel]
 * (q adds the state and actions there).
 */
@Composable
internal fun AccountsSection(state: SettingsUiState, vm: SettingsViewModel) {
    Text("AccountsSection")
}
