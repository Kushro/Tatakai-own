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
import androidx.compose.runtime.collectAsState
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
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.ProviderStatus
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.R as UiR
import org.koin.compose.koinInject
import kotlinx.coroutines.flow.emptyFlow

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
        // parity wave B (16 §3.1, worker r): how adult titles are shown once they are let in (checklist settings.privacy.mature-content).
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.search_settings_mature), style = MaterialTheme.typography.bodyLarge)
            Text(
                stringResource(R.string.search_settings_mature_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Segmented(
                options = MatureContentMode.entries.map { SegmentedOption(it, stringResource(it.labelRes())) },
                selected = settings.matureContent,
                onSelect = { mode -> onUpdate { it.copy(matureContent = mode) } },
            )
        }
        // parity wave B (16 §3.1, worker r): enable the six providers + their live status (checklist settings.metadata.extra-providers /
        // settings.metadata.provider-status). Order is edited by the priority list above (q's rows); disabled = absent from the priority.
        MetadataProviderRows(settings, onUpdate)
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

@Composable
private fun MetadataProviderRows(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit) {
    val metadata = koinInject<MetadataRepository>()
    val flow = remember(metadata) { runCatching { metadata.providerStatus() }.getOrDefault(emptyFlow()) }
    val statuses by flow.collectAsState(initial = emptyList())
    val now = remember(statuses) { System.currentTimeMillis() }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.search_settings_meta_providers), style = MaterialTheme.typography.bodyLarge)
        Text(
            stringResource(R.string.search_settings_meta_providers_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        AppSettings.KNOWN_PROVIDER_IDS.forEach { id ->
            val enabled = id in settings.providerPriority
            val status = statuses.firstOrNull { it.id == id }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f)) {
                    Text(metaProviderName(id), style = MaterialTheme.typography.bodyLarge)
                    Text(
                        providerStatusText(enabled, status, now),
                        style = MaterialTheme.typography.bodySmall,
                        color = if (status?.lastError != null && enabled) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = enabled,
                    // AniList stays the base of discovery: at least one provider is always on.
                    enabled = !(enabled && settings.providerPriority.size <= 1),
                    onCheckedChange = { on -> onUpdate { it.copy(providerPriority = ProviderToggle.apply(it.providerPriority, id, on)) } },
                )
            }
        }
    }
}

/** Pure rule of the provider switches (JVM-tested): enabling appends at the end, disabling removes; never empties the list. */
internal object ProviderToggle {
    fun apply(priority: List<String>, id: String, enabled: Boolean): List<String> = when {
        enabled && id !in priority -> priority + id
        !enabled && priority.size > 1 -> priority - id
        else -> priority
    }

    /** Whole minutes left of a cooldown, at least 1. */
    fun minutesLeft(untilMs: Long, nowMs: Long): Long = ((untilMs - nowMs + 59_999) / 60_000).coerceAtLeast(1)
}

@Composable
private fun providerStatusText(enabled: Boolean, status: ProviderStatus?, now: Long): String {
    if (!enabled) return stringResource(R.string.search_settings_meta_off)
    val cooldown = status?.cooldownUntilMs
    return when {
        cooldown != null && cooldown > now -> stringResource(R.string.search_settings_meta_paused, ProviderToggle.minutesLeft(cooldown, now).toInt())
        status?.lastError != null -> stringResource(R.string.search_settings_meta_error, status.lastError!!)
        status?.lastOkAtMs != null -> stringResource(R.string.search_settings_meta_ok)
        else -> stringResource(R.string.search_settings_meta_idle)
    }
}

private fun metaProviderName(id: String): String = when (id) {
    "anilist" -> "AniList"
    "anizip" -> "ani.zip"
    "kitsu" -> "Kitsu"
    "mal" -> "MyAnimeList"
    "shikimori" -> "Shikimori"
    "tenrai" -> "Tenrai"
    else -> id
}

internal fun MatureContentMode.labelRes(): Int = when (this) {
    MatureContentMode.SHOW -> R.string.search_settings_mature_show
    MatureContentMode.BLUR -> R.string.search_settings_mature_blur
    MatureContentMode.WARN -> R.string.search_settings_mature_warn
}

internal fun TitleLanguage.labelRes(): Int = when (this) {
    TitleLanguage.ROMAJI -> R.string.search_title_romaji
    TitleLanguage.ENGLISH -> R.string.search_title_english
    TitleLanguage.NATIVE -> R.string.search_title_native
}
