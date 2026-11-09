// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.4, 2026-11-09 amendment, checklist system.reset-app).
 * Owner: worker q. Mounted at the end of `DataSection`.
 *
 * "Restablecer la app": outlined error-coloured button (`settings_data_reset`, hint `settings_data_reset_hint`) → `AlertDialog` with
 * `settings_data_reset_confirm` / cancel → [SettingsViewModel.resetApp]; while `SettingsUiState.resetting` the button shows a
 * `LoadingIndicator`. On `SettingsEvent.ResetDone` the screen recreates the activity (`(context as Activity).recreate()`), which lands on
 * the first-run wizard. q also puts "Exportar historial" and "Borrar historial" (`HistoryRepository.exportJson` / `clearHistory`) next to it.
 */
@Composable
internal fun ResetAppRow(vm: SettingsViewModel) {
    Text("ResetAppRow")
}
