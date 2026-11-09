// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.AppSettings

/**
 * Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.2). Owner: worker p. Mounted by Settings › Reproductor
 * (worker q's module) right after the "Saltar intro" switch, so p owns its own settings rows AND their strings (`player_settings_*` in
 * :feature:player) without touching :feature:settings.
 *
 * Rows p renders here: subtitle translation (provider, mode, target language, endpoint / key / model / region, "Probar"), secondary
 * subtitle language, auto-next countdown seconds (3..60), stable volume, background playback, keyboard shortcuts editor
 * (`AppSettings.playerKeymap`). Every change goes through [onUpdate] (saves immediately and shows "Ajustes guardados").
 */
@Composable
fun PlayerSettingsExtras(
    settings: AppSettings,
    onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("PlayerSettingsExtras", modifier)
}
