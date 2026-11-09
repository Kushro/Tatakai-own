// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.R as UiR
import org.koin.compose.koinInject

/**
 * Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.1). Owner: worker o. Mounted by Settings › Metadatos
 * (worker q's module) after the cache rows, so o owns these rows AND their strings (`search_settings_*` in :feature:search).
 *
 * Rows: preferred title language (`AppSettings.titleLanguage`: Romaji / English / Native), 18+ content (`AppSettings.showAdult`: the
 * initial value of the Search toggle; Details still asks once per adult title), "Borrar búsquedas recientes" (SearchHistoryStore).
 */
@Composable
fun DiscoverSettingsExtras(
    settings: AppSettings,
    onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit,
    modifier: Modifier = Modifier,
) {
    val history = koinInject<SearchHistoryStore>()
    val snackbar = LocalAppSnackbar.current
    val cleared = stringResource(R.string.search_settings_history_cleared)
    var confirm by rememberSaveable { mutableStateOf(false) }
    // Re-read when the dialog closes so the count follows a clear.
    val count = remember(confirm) { history.load().size }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.search_settings_title_language), style = MaterialTheme.typography.bodyLarge)
            Text(
                stringResource(R.string.search_settings_title_language_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Segmented(
                options = TitleLanguage.entries.map { SegmentedOption(it, stringResource(it.labelRes())) },
                selected = settings.titleLanguage,
                onSelect = { lang -> onUpdate { it.copy(titleLanguage = lang) } },
            )
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.search_settings_adult), style = MaterialTheme.typography.bodyLarge)
                Text(
                    stringResource(R.string.search_settings_adult_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Switch(checked = settings.showAdult, onCheckedChange = { on -> onUpdate { it.copy(showAdult = on) } })
        }
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.search_settings_history), style = MaterialTheme.typography.bodyLarge)
                Text(
                    if (count == 0) {
                        stringResource(R.string.search_settings_history_empty)
                    } else {
                        stringResource(R.string.search_settings_history_count, count)
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            OutlinedButton(onClick = { confirm = true }, enabled = count > 0) {
                Icon(Icons.Outlined.DeleteSweep, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.search_settings_history_clear), modifier = Modifier.padding(start = 6.dp))
            }
        }
    }
    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.search_settings_history_clear)) },
            text = { Text(stringResource(R.string.search_settings_history_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    history.save(emptyList())
                    confirm = false
                    snackbar.show(cleared)
                }) { Text(stringResource(R.string.search_settings_history_clear)) }
            },
            dismissButton = { TextButton(onClick = { confirm = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
}

internal fun TitleLanguage.labelRes(): Int = when (this) {
    TitleLanguage.ROMAJI -> R.string.search_title_romaji
    TitleLanguage.ENGLISH -> R.string.search_title_english
    TitleLanguage.NATIVE -> R.string.search_title_native
}
