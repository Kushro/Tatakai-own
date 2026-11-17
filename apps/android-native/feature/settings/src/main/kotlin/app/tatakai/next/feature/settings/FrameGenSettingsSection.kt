// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.FileOpen
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.framegen.FgProfile
import app.tatakai.next.framegen.FgUnavailableReason
import app.tatakai.next.framegen.FrameGenController
import app.tatakai.next.framegen.FrameGenMultiplier
import app.tatakai.next.framegen.FrameGenText
import app.tatakai.next.framegen.FrameGenUiState
import app.tatakai.next.framegen.R as FgR
import kotlinx.coroutines.launch
import org.koin.compose.getKoin
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle

/**
 * 0.4.0 (worker B): Settings › Reproductor › «Generación de cuadros en el teléfono»: the state on this device, the Lossless.dll
 * import (SAF picker -> SPIR-V shaders in app storage) / removal, and the global FG choices. Self-contained: it talks only to
 * the app-wide [FrameGenController] (:core:framegen) and renders nothing when FG is not wired. The in-player controls (worker
 * D) use the same controller and the same strings ([FrameGenText]).
 */
@Composable
internal fun FrameGenSettingsSection() {
    val koin = getKoin()
    val controller = remember { koin.getOrNull<FrameGenController>() } ?: return
    val state by controller.state.collectAsState()
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(controller) { controller.refresh() }

    val pickDll = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri != null && !busy) {
            busy = true
            scope.launch {
                val result = runCatching { controller.importShaders(uri) }.getOrNull()
                busy = false
                val message = result?.let { FrameGenText.install(it) } ?: FgR.string.fg_install_write_failed
                snackbar.show(context.getString(message))
            }
        }
    }

    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
        Text(
            stringResource(FgR.string.fg_title),
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(top = 12.dp).semantics { heading() },
        )
        Text(
            stringResource(FgR.string.fg_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(
            frameGenStatusText(state),
            style = MaterialTheme.typography.bodyMedium,
            color = if (state.unavailable != null) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(vertical = 4.dp),
        )

        // Lossless.dll (the shaders are not redistributable: the user imports their own copy).
        val libraryMissing = state.unavailable == FgUnavailableReason.ABI || state.unavailable == FgUnavailableReason.LIBRARY
        val shadersLine = when {
            busy -> stringResource(FgR.string.fg_shaders_importing)
            state.shadersImported == true ->
                state.shadersImportedAt?.let { stringResource(FgR.string.fg_shaders_imported, formatImportedAt(it)) }
                    ?: stringResource(FgR.string.fg_shaders_imported_plain)
            else -> stringResource(FgR.string.fg_shaders_missing)
        }
        SettingRow(stringResource(FgR.string.fg_shaders), stringResource(FgR.string.fg_shaders_hint) + "\n" + shadersLine, stack = true) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = { pickDll.launch(arrayOf("*/*")) }, enabled = !busy && !libraryMissing) {
                    Icon(Icons.Outlined.FileOpen, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        stringResource(if (state.shadersImported == true) FgR.string.fg_shaders_reimport else FgR.string.fg_shaders_import),
                        Modifier.padding(start = 6.dp),
                    )
                }
                if (state.shadersImported == true) {
                    TextButton(
                        onClick = {
                            busy = true
                            scope.launch {
                                val removed = runCatching { controller.removeShaders() }.getOrDefault(false)
                                busy = false
                                if (removed) snackbar.show(context.getString(FgR.string.fg_shaders_removed))
                            }
                        },
                        enabled = !busy,
                    ) {
                        Icon(Icons.Outlined.Delete, contentDescription = null, modifier = Modifier.size(18.dp))
                        Text(stringResource(FgR.string.fg_shaders_remove), Modifier.padding(start = 6.dp))
                    }
                }
            }
        }

        val canEnable = state.enabled || state.unavailable == null || state.unavailable?.kind == FgUnavailableReason.Kind.SUSPENDED
        SwitchRow(
            label = stringResource(FgR.string.fg_enabled),
            checked = state.enabled,
            onChange = controller::setEnabled,
            hint = stringResource(FgR.string.fg_where_phone),
            enabled = canEnable,
        )
        SettingRow(stringResource(FgR.string.fg_multiplier), stringResource(FgR.string.fg_multiplier_hint), stack = true) {
            Segmented(
                options = FrameGenMultiplier.entries.map { m ->
                    val label = if (m == FrameGenMultiplier.AUTO) {
                        stringResource(FgR.string.fg_multiplier_auto)
                    } else {
                        stringResource(FgR.string.fg_multiplier_n, m.n)
                    }
                    SegmentedOption(m, label)
                },
                selected = FrameGenMultiplier.of(state.settings.multiplier),
                onSelect = controller::setMultiplier,
                small = true,
                fillWidth = true,
            )
        }
        SettingRow(stringResource(FgR.string.fg_profile), stringResource(FgR.string.fg_profile_hint), stack = true) {
            Segmented(
                options = listOf(FgProfile.SAVER, FgProfile.BALANCED, FgProfile.QUALITY).map {
                    SegmentedOption(it, stringResource(FrameGenText.profile(it)))
                },
                selected = state.profile,
                onSelect = controller::setProfile,
                fillWidth = true,
            )
        }
        SwitchRow(
            label = stringResource(FgR.string.fg_match_refresh),
            checked = state.settings.matchRefresh,
            onChange = controller::setMatchRefresh,
            hint = stringResource(FgR.string.fg_match_refresh_hint),
        )
        SwitchRow(
            label = stringResource(FgR.string.fg_allow_untested),
            checked = state.settings.allowUntestedVulkan,
            onChange = controller::setAllowUntestedGpu,
            hint = stringResource(FgR.string.fg_allow_untested_hint),
        )
        SwitchRow(
            label = stringResource(FgR.string.fg_show_stats),
            checked = state.settings.showStats,
            onChange = controller::setShowStats,
        )
    }
}

/** One line: active (N×), ready, the reason it cannot run, or still checking. */
@Composable
private fun frameGenStatusText(state: FrameGenUiState): String {
    val reason = state.unavailable
    return when {
        state.active -> stringResource(FgR.string.fg_state_active, state.multiplier)
        reason != null -> stringResource(FrameGenText.reason(reason, state.noInputTimeout))
        !state.eligibilityKnown -> stringResource(FgR.string.fg_state_checking)
        state.experimental -> stringResource(FgR.string.fg_state_ready) + " · " + stringResource(FgR.string.fg_state_experimental)
        else -> stringResource(FgR.string.fg_state_ready)
    }
}

private fun formatImportedAt(iso: String): String =
    runCatching {
        DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM).withZone(ZoneId.systemDefault()).format(Instant.parse(iso))
    }.getOrDefault(iso)
