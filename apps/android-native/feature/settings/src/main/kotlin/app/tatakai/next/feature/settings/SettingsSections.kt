@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.settings

import android.content.Context
import android.os.Build
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material.icons.outlined.ClosedCaption
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.generated.PaletteInfo
import app.tatakai.next.designsystem.generated.TatakaiTokens
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.feature.player.PlayerSettingsExtras
import app.tatakai.next.feature.search.DiscoverSettingsExtras
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.model.ThemeMode
import app.tatakai.next.model.torrent.StorageLocation
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.util.Locale
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

// ==================================================================== Apariencia

@Composable
internal fun AppearanceSection(state: SettingsUiState, vm: SettingsViewModel) {
    val s = state.settings
    SettingRow(stringResource(R.string.settings_palette), stringResource(R.string.settings_palette_hint), stack = true, divider = false) {
        LazyRow(contentPadding = PaddingValues(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(TatakaiTokens.paletteInfo, key = { it.id }, contentType = { "palette" }) { info ->
                PaletteSwatch(
                    info,
                    selected = info.id == s.paletteId && !(s.dynamicColor && dynamicSupported),
                    onClick = { vm.setPalette(info.id) },
                )
            }
        }
    }
    if (dynamicSupported) {
        SwitchRow(stringResource(R.string.settings_dynamic), s.dynamicColor, vm::setDynamicColor, hint = stringResource(R.string.settings_dynamic_hint))
    }
    SettingRow(stringResource(R.string.settings_mode), stack = true) {
        Segmented(
            options = listOf(
                SegmentedOption(ThemeMode.SYSTEM, stringResource(R.string.settings_mode_system)),
                SegmentedOption(ThemeMode.LIGHT, stringResource(R.string.settings_mode_light)),
                SegmentedOption(ThemeMode.DARK, stringResource(R.string.settings_mode_dark)),
            ),
            selected = s.themeMode,
            onSelect = vm::setThemeMode,
        )
    }
    SwitchRow(stringResource(R.string.settings_amoled), s.amoled, vm::setAmoled, hint = stringResource(R.string.settings_amoled_hint))
    SettingRow(stringResource(R.string.settings_contrast), stack = true) {
        Segmented(
            options = ContrastPref.entries.map { SegmentedOption(it, stringResource(it.labelRes())) },
            selected = s.contrast,
            onSelect = vm::setContrast,
        )
    }
    SettingRow(stringResource(R.string.settings_language), stack = true) {
        Segmented(
            options = listOf(
                SegmentedOption(AppLanguage.SYSTEM, stringResource(R.string.settings_lang_system)),
                SegmentedOption(AppLanguage.EN, stringResource(R.string.settings_lang_en)),
                SegmentedOption(AppLanguage.ES, stringResource(R.string.settings_lang_es)),
            ),
            selected = s.language,
            onSelect = vm::setLanguage,
        )
    }
    SwitchRow(
        stringResource(R.string.settings_reduce_motion),
        s.reduceMotion,
        vm::setReduceMotion,
        hint = stringResource(R.string.settings_reduce_motion_hint),
    )
}

private val dynamicSupported: Boolean get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S

private fun ContrastPref.labelRes(): Int = when (this) {
    ContrastPref.STANDARD -> R.string.settings_contrast_standard
    ContrastPref.MEDIUM -> R.string.settings_contrast_medium
    ContrastPref.HIGH -> R.string.settings_contrast_high
}

@Composable
private fun PaletteSwatch(info: PaletteInfo, selected: Boolean, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(
            Modifier
                .size(48.dp)
                .clip(CircleShape)
                .background(info.seed)
                .then(if (selected) Modifier.border(BorderStroke(3.dp, MaterialTheme.colorScheme.primary), CircleShape) else Modifier)
                .clickable(onClick = onClick),
            contentAlignment = Alignment.Center,
        ) {
            if (selected) Icon(Icons.Filled.Check, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimary)
        }
        Text(info.label, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(top = 4.dp))
    }
}

// ==================================================================== Reproductor

/** Prototype data.js PRESETS (only the mpv engine applies them, 12 §7.13). */
private val PRESETS = listOf("off", "anime4k-a", "anime4k-aa", "anime4k-b", "anime4k-c", "fsr")

private const val PKG_MPV = "is.xyz.mpv"
private const val PKG_VLC = "org.videolan.vlc"

@Composable
internal fun PlayerSection(state: SettingsUiState, vm: SettingsViewModel, onOpenExtensions: () -> Unit) {
    val s = state.settings
    val prefs = s.sourcePrefs
    var editingSubs by rememberSaveable { mutableStateOf(false) }

    SettingRow(stringResource(R.string.settings_engine), stringResource(s.playerEngine.hintRes()), stack = true, divider = false) {
        Segmented(
            options = listOf(
                SegmentedOption(PlayerEngine.MEDIA3, stringResource(R.string.settings_engine_media3)),
                SegmentedOption(PlayerEngine.MPV, stringResource(R.string.settings_engine_mpv)),
                SegmentedOption(PlayerEngine.EXTERNAL, stringResource(R.string.settings_engine_external)),
            ),
            selected = s.playerEngine,
            onSelect = vm::setPlayerEngine,
        )
    }
    if (s.playerEngine == PlayerEngine.EXTERNAL) {
        SettingRow(stringResource(R.string.settings_external_app), stringResource(R.string.settings_external_app_hint), stack = true) {
            Segmented(
                options = listOf<SegmentedOption<String?>>(
                    SegmentedOption(null, stringResource(R.string.settings_external_chooser)),
                    SegmentedOption(PKG_MPV, stringResource(R.string.settings_external_mpv)),
                    SegmentedOption(PKG_VLC, stringResource(R.string.settings_external_vlc)),
                ),
                selected = s.externalPlayerPackage,
                onSelect = vm::setExternalPlayerPackage,
                small = true,
            )
        }
    }
    val presetHint = buildString {
        append(stringResource(R.string.settings_preset_battery))
        if (s.playerEngine != PlayerEngine.MPV) append(' ').append(stringResource(R.string.settings_preset_mpv_only))
    }
    SettingRow(stringResource(R.string.settings_preset), presetHint, stack = true) {
        Segmented(
            options = PRESETS.map { SegmentedOption(it, stringResource(presetLabelRes(it)), description = stringResource(presetHintRes(it))) },
            selected = s.playerPreset,
            onSelect = vm::setPlayerPreset,
            small = true,
        )
    }
    SettingRow(stringResource(R.string.settings_source_mode), stringResource(R.string.settings_source_mode_hint), stack = true) {
        Segmented(
            options = listOf(
                SegmentedOption(SourceMode.AUTO, stringResource(UiR.string.src_mode_auto)),
                SegmentedOption(SourceMode.MANUAL, stringResource(UiR.string.src_mode_manual)),
            ),
            selected = prefs.mode,
            onSelect = vm::setSourceMode,
        )
    }
    SettingRow(stringResource(R.string.settings_prefer), stack = true) {
        Segmented(
            options = listOf<SegmentedOption<SourceKind?>>(
                SegmentedOption(null, stringResource(R.string.settings_prefer_any)),
                SegmentedOption(SourceKind.WEB, stringResource(UiR.string.src_web)),
                SegmentedOption(SourceKind.TORRENT, stringResource(UiR.string.src_torrent)),
            ),
            selected = prefs.prefer,
            onSelect = vm::setPreferredKind,
            small = true,
        )
    }
    SettingRow(stringResource(R.string.settings_min_quality), stack = true) {
        Segmented(
            options = listOf(Quality.P480, Quality.P720, Quality.P1080).map { SegmentedOption(it, it.label()) },
            selected = prefs.minQuality,
            onSelect = vm::setMinQuality,
            small = true,
        )
    }
    SettingRow(stringResource(R.string.settings_sub_lang), stack = true) {
        Segmented(options = langOptions(listOf("es", "en", "pt")), selected = prefs.subLang, onSelect = vm::setSubLang, small = true)
    }
    SettingRow(stringResource(R.string.settings_dub_lang), stack = true) {
        Segmented(options = langOptions(listOf("ja", "es", "en")), selected = prefs.dubLang, onSelect = vm::setDubLang, small = true)
    }
    SwitchRow(stringResource(R.string.settings_autonext), s.autoNext, vm::setAutoNext, hint = stringResource(R.string.settings_autonext_hint))
    SwitchRow(stringResource(R.string.settings_skip), s.skipIntro, vm::setSkipIntro, hint = stringResource(R.string.settings_skip_hint))
    // parity wave A (15 §3.2): worker p's rows (translation, secondary subtitle, countdown, stable volume, background playback, shortcuts).
    PlayerSettingsExtras(settings = s, onUpdate = vm::updateSettings)
    SettingRow(stringResource(R.string.settings_subs), subtitleSummary(s.subtitleStyle)) {
        FilledTonalButton(onClick = { editingSubs = true }) {
            Icon(Icons.Outlined.ClosedCaption, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.settings_subs_edit), modifier = Modifier.padding(start = 8.dp))
        }
    }
    NavRow(Icons.Outlined.Extension, stringResource(R.string.settings_extensions), onOpenExtensions)
    if (editingSubs) {
        SubtitleStyleSheet(style = s.subtitleStyle, onChange = vm::setSubtitleStyle, onDismiss = { editingSubs = false })
    }
}

private fun PlayerEngine.hintRes(): Int = when (this) {
    PlayerEngine.MEDIA3 -> R.string.settings_engine_media3_hint
    PlayerEngine.MPV -> R.string.settings_engine_mpv_hint
    PlayerEngine.EXTERNAL -> R.string.settings_engine_external_hint
}

private fun presetLabelRes(id: String): Int = when (id) {
    "anime4k-a" -> R.string.settings_preset_anime4k_a
    "anime4k-aa" -> R.string.settings_preset_anime4k_aa
    "anime4k-b" -> R.string.settings_preset_anime4k_b
    "anime4k-c" -> R.string.settings_preset_anime4k_c
    "fsr" -> R.string.settings_preset_fsr
    else -> R.string.settings_preset_off
}

private fun presetHintRes(id: String): Int = when (id) {
    "anime4k-a", "anime4k-aa" -> R.string.settings_preset_hint_restore
    "anime4k-b", "anime4k-c" -> R.string.settings_preset_hint_soft
    "fsr" -> R.string.settings_preset_hint_fsr
    else -> R.string.settings_preset_hint_off
}

/** ISO 639-1 codes shown as upper-case codes (technical, not translated). */
private fun langOptions(codes: List<String>): List<SegmentedOption<String>> =
    codes.map { SegmentedOption(it, it.uppercase(Locale.ROOT)) }

@Composable
private fun subtitleSummary(style: SubtitleStyle): String = stringResource(
    R.string.settings_subs_summary,
    stringResource(fontLabelRes(style.font)),
    (style.scale * 100).toInt(),
    (style.backgroundOpacity * 100).toInt(),
)

private fun fontLabelRes(font: String): Int = when (font) {
    "space_grotesk" -> R.string.settings_subs_font_space_grotesk
    "system" -> R.string.settings_subs_font_system
    else -> R.string.settings_subs_font_outfit
}

@Composable
private fun SubtitleStyleSheet(style: SubtitleStyle, onChange: (SubtitleStyle) -> Unit, onDismiss: () -> Unit) {
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(stringResource(R.string.settings_subs), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(bottom = 8.dp))
            SubtitlePreview(style)
            SettingRow(stringResource(R.string.settings_subs_size), stack = true, divider = false) {
                Segmented(
                    options = listOf(0.8f, 1f, 1.3f).map { SegmentedOption(it, "${(it * 100).toInt()} %") },
                    selected = style.scale,
                    onSelect = { onChange(style.copy(scale = it)) },
                    small = true,
                )
            }
            SettingRow(stringResource(R.string.settings_subs_background), stack = true) {
                Segmented(
                    options = listOf(0f, 0.6f, 1f).map { SegmentedOption(it, "${(it * 100).toInt()} %") },
                    selected = style.backgroundOpacity,
                    onSelect = { onChange(style.copy(backgroundOpacity = it)) },
                    small = true,
                )
            }
            SettingRow(stringResource(R.string.settings_subs_font), stack = true) {
                Segmented(
                    options = listOf("outfit", "space_grotesk", "system").map { SegmentedOption(it, stringResource(fontLabelRes(it))) },
                    selected = style.font,
                    onSelect = { onChange(style.copy(font = it)) },
                    small = true,
                )
            }
            SwitchRow(stringResource(R.string.settings_subs_bold), style.bold, { onChange(style.copy(bold = it)) })
            SwitchRow(stringResource(R.string.settings_subs_outline), style.outline, { onChange(style.copy(outline = it)) })
        }
    }
}

/** Live sample of the subtitle look on an inverse surface (stand-in for video). */
@Composable
private fun SubtitlePreview(style: SubtitleStyle) {
    val family = when (style.font) {
        "space_grotesk" -> TatakaiFonts.spaceGrotesk
        "system" -> FontFamily.Default
        else -> TatakaiFonts.outfit
    }
    Box(
        Modifier
            .fillMaxWidth()
            .height(120.dp)
            .clip(MaterialTheme.shapes.large)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.BottomCenter,
    ) {
        Text(
            stringResource(R.string.settings_subs_preview),
            style = TextStyle(
                fontFamily = family,
                fontWeight = if (style.bold) FontWeight.Bold else FontWeight.Medium,
                fontSize = (16 * style.scale).sp,
                textAlign = TextAlign.Center,
            ),
            color = MaterialTheme.colorScheme.inverseOnSurface,
            modifier = Modifier
                .padding(12.dp)
                .clip(MaterialTheme.shapes.small)
                .background(MaterialTheme.colorScheme.inverseSurface.copy(alpha = style.backgroundOpacity.coerceAtLeast(0.15f)))
                .padding(horizontal = 8.dp, vertical = 4.dp),
        )
    }
}

// ==================================================================== Metadatos

private val TTL_HOURS = listOf(6, 24, 72, 168)

@Composable
internal fun MetadataSection(state: SettingsUiState, vm: SettingsViewModel) {
    val priority = state.settings.providerPriority
    SectionHint(stringResource(R.string.settings_metadata_hint))
    priority.forEachIndexed { index, id ->
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            Text(
                "${index + 1}",
                style = MaterialTheme.typography.titleMedium.copy(fontFamily = TatakaiFonts.jetBrainsMono),
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(end = 12.dp),
            )
            Column(Modifier.weight(1f)) {
                Text(providerName(id), style = MaterialTheme.typography.bodyLarge)
                providerSubRes(id)?.let {
                    Text(stringResource(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            IconButton(onClick = { vm.moveProvider(id, -1) }, enabled = index > 0) {
                Icon(Icons.Filled.KeyboardArrowUp, contentDescription = stringResource(R.string.settings_move_up_named, providerName(id)))
            }
            IconButton(onClick = { vm.moveProvider(id, 1) }, enabled = index < priority.lastIndex) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.settings_move_down_named, providerName(id)))
            }
        }
    }
    SettingRow(stringResource(R.string.settings_metadata_cache), cacheSummary(state.storage)) {}
    SettingRow(stringResource(R.string.settings_metadata_ttl), stringResource(R.string.settings_metadata_ttl_hint), stack = true) {
        Segmented(
            options = TTL_HOURS.map { h ->
                SegmentedOption(
                    h,
                    if (h < 48) stringResource(R.string.settings_ttl_hours, h) else stringResource(R.string.settings_ttl_days, h / 24),
                )
            },
            selected = state.settings.metadataCacheTtlHours,
            onSelect = vm::setMetadataTtl,
            small = true,
        )
    }
    // parity wave A (15 §3.1): worker o's rows (title language, 18+ content, clear recent searches).
    DiscoverSettingsExtras(settings = state.settings, onUpdate = vm::updateSettings)
}

@Composable
private fun cacheSummary(storage: LoadState<StorageInfo>): String = when (storage) {
    LoadState.Loading -> stringResource(R.string.settings_measuring)
    is LoadState.Err -> stringResource(R.string.settings_unknown_size)
    is LoadState.Ok -> storage.value.cache?.let { stringResource(R.string.settings_cache_value, formatBytes(it.bytes), it.entries) }
        ?: stringResource(R.string.settings_unknown_size)
}

/** Proper names of the metadata providers (not translated). */
private fun providerName(id: String): String = when (id) {
    "anilist" -> "AniList"
    "anizip" -> "ani.zip"
    "kitsu" -> "Kitsu"
    else -> id
}

private fun providerSubRes(id: String): Int? = when (id) {
    "anilist" -> R.string.settings_provider_anilist_sub
    "anizip" -> R.string.settings_provider_anizip_sub
    "kitsu" -> R.string.settings_provider_kitsu_sub
    else -> null
}

// ==================================================================== Enlace / Torrents

/**
 * Enlace: the linked-device entry plus, since the 2026-11-09 amendment (no own login), the history sync between the user's devices over
 * Tatakai Link (`AppSettings.historySyncOverLink`, last sync stamp, "Sincronizar ahora"). Owner: worker q.
 */
@Composable
internal fun LinkSection(state: SettingsUiState, vm: SettingsViewModel, onOpenLinked: () -> Unit) {
    SectionHint(stringResource(R.string.settings_link_hint))
    FilledTonalButton(onClick = onOpenLinked) {
        Icon(Icons.Outlined.Computer, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        Text(stringResource(R.string.settings_link_open), modifier = Modifier.padding(start = 8.dp))
    }
    Text(
        stringResource(R.string.settings_link_history_title),
        style = MaterialTheme.typography.bodyLarge,
        modifier = Modifier.padding(top = 16.dp),
    )
    SectionHint(stringResource(R.string.settings_link_history_hint))
    SwitchRow(
        label = stringResource(R.string.settings_link_history_switch),
        checked = state.settings.historySyncOverLink,
        onChange = { on -> vm.updateSettings { it.copy(historySyncOverLink = on) } },
    )
    Text(
        when {
            !state.linkPaired -> stringResource(R.string.settings_link_history_unpaired)
            state.lastSyncedAt != null -> stringResource(R.string.settings_account_synced, relativeTime(state.lastSyncedAt))
            else -> stringResource(R.string.settings_account_never)
        },
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
    )
    FilledTonalButton(
        onClick = vm::syncNow,
        enabled = state.linkPaired && state.settings.historySyncOverLink && !state.syncing,
        modifier = Modifier.padding(top = 8.dp),
    ) {
        if (state.syncing) {
            LoadingIndicator(Modifier.size(20.dp))
        } else {
            Icon(Icons.Outlined.Sync, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        }
        Text(
            stringResource(if (state.syncing) R.string.settings_syncing else R.string.settings_sync_now),
            modifier = Modifier.padding(start = 8.dp),
        )
    }
}

@Composable
internal fun TorrentsSection(onOpenTorrents: () -> Unit) {
    SectionHint(stringResource(R.string.settings_torrents_hint))
    NavRow(
        Icons.Outlined.Download,
        stringResource(R.string.settings_torrents_open),
        onOpenTorrents,
        supporting = stringResource(R.string.settings_torrents_open_sub),
    )
}

// ==================================================================== Datos y almacenamiento

@Composable
internal fun DataSection(state: SettingsUiState, vm: SettingsViewModel) {
    var confirmClear by rememberSaveable { mutableStateOf(false) }
    SectionHint(stringResource(R.string.settings_data_hint))
    when (val storage = state.storage) {
        LoadState.Loading -> if (rememberAfterDelay(150)) {
            SkeletonBox(Modifier.fillMaxWidth().height(12.dp), shape = CircleShape)
            SkeletonBox(Modifier.fillMaxWidth(0.7f).height(16.dp))
        }
        is LoadState.Err -> ErrorState(
            message = storage.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = vm::refreshStorage,
            title = stringResource(R.string.settings_storage_error),
            compact = true,
        )
        is LoadState.Ok -> StorageBreakdown(storage.value)
    }
    SettingRow(stringResource(R.string.settings_telemetry), stringResource(R.string.settings_telemetry_hint)) {
        Pill(stringResource(R.string.settings_telemetry_none), tone = PillTone.OK)
    }
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
        OutlinedButton(onClick = { confirmClear = true }, enabled = !state.clearingCache) {
            if (state.clearingCache) {
                LoadingIndicator(Modifier.size(20.dp))
            } else {
                Icon(Icons.Outlined.Delete, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            }
            Text(stringResource(R.string.settings_clear_cache), modifier = Modifier.padding(start = 8.dp))
        }
    }
    if (confirmClear) {
        val bytes = (state.storage as? LoadState.Ok)?.value?.clearableBytes
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            icon = { Icon(Icons.Outlined.Delete, contentDescription = null) },
            title = { Text(stringResource(R.string.settings_clear_cache)) },
            text = {
                Text(
                    if (bytes != null) {
                        stringResource(R.string.settings_clear_cache_body, formatBytes(bytes))
                    } else {
                        stringResource(R.string.settings_clear_cache_body_unknown)
                    },
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmClear = false
                        vm.clearCache()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text(stringResource(R.string.settings_clear_cache)) }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
    // parity wave A, 2026-11-09 amendment (worker q): Exportar historial, Borrar historial and Restablecer la app live here.
    ResetAppRow(vm)
}

/** Storage bar (prototype `.storage__bar`): metadata cache, torrents, free space; figures in the mono font. */
@Composable
private fun StorageBreakdown(info: StorageInfo) {
    // metadata table + image disk cache (both go with "Limpiar caché"), shown as one slice and two legend rows
    val cache = info.clearableBytes ?: 0L
    val torrents = info.torrents?.usedBytes ?: 0L
    val free = info.torrents?.freeBytes ?: 0L
    val total = (cache + torrents + free).coerceAtLeast(1L)
    val cacheColor = MaterialTheme.colorScheme.primary
    val torrentColor = MaterialTheme.colorScheme.tertiary
    val freeColor = MaterialTheme.colorScheme.surfaceContainerHighest
    Row(Modifier.fillMaxWidth().height(12.dp).clip(CircleShape).background(freeColor)) {
        if (cache > 0) Box(Modifier.weight(cache.toFloat() / total).height(12.dp).background(cacheColor))
        if (torrents > 0) Box(Modifier.weight(torrents.toFloat() / total).height(12.dp).background(torrentColor))
        val rest = (total - cache - torrents).coerceAtLeast(0L)
        if (rest > 0) Box(Modifier.weight(rest.toFloat() / total).height(12.dp))
    }
    Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        LegendRow(cacheColor, stringResource(R.string.settings_data_cache), info.cache?.let { formatBytes(it.bytes) })
        LegendRow(cacheColor, stringResource(R.string.settings_data_images), info.imageBytes?.let { formatBytes(it) })
        LegendRow(torrentColor, stringResource(R.string.settings_data_torrents), info.torrents?.let { formatBytes(it.usedBytes) })
        LegendRow(freeColor, stringResource(R.string.settings_data_free), info.torrents?.let { formatBytes(it.freeBytes) })
        info.torrents?.location?.let { location ->
            Text(
                stringResource(
                    if (location is StorageLocation.UserFolder) R.string.settings_data_location_folder else R.string.settings_data_location_app,
                ),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun LegendRow(color: Color, label: String, value: String?) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(color))
        Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        Text(
            value ?: stringResource(R.string.settings_unknown_size),
            style = MaterialTheme.typography.bodyMedium.copy(fontFamily = TatakaiFonts.jetBrainsMono),
        )
    }
}

// ==================================================================== Acerca de

/** Versions of what is bundled (technical strings, not translated). */
private val COMPONENT_VERSIONS = listOf(
    "Media3" to "1.11.1",
    "libmpv" to "1.0.0 (dev.jdtech.mpv)",
    "libtorrent4j" to "2.1.0-39",
    "Tatakai Link" to "v1 · openapi.v1.json",
)

@Composable
internal fun AboutSection(state: SettingsUiState, vm: SettingsViewModel) {
    val context = LocalContext.current
    val system: SystemViewModel = koinViewModel()
    var showLicenses by rememberSaveable { mutableStateOf(false) }
    var showLogs by rememberSaveable { mutableStateOf(false) }
    val version = produceState(initialValue = "") {
        value = runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull().orEmpty()
    }.value
    SectionHint(stringResource(R.string.settings_about_body))
    AboutRow("Tatakai Next", listOf(version, context.packageName).filter(String::isNotBlank).joinToString(" · "))
    COMPONENT_VERSIONS.forEach { (name, v) -> AboutRow(name, v) }
    AboutRow(stringResource(R.string.settings_about_license), "AGPL-3.0-or-later")
    // parity wave A (worker q, checklist system.update-check): GitHub Releases of Kushro/Tatakai-own, at most once a day on its own.
    UpdateCheckRow(
        system = system,
        updateCheckEnabled = state.settings.updateCheck,
        onToggle = { on -> vm.updateSettings { it.copy(updateCheck = on) } },
    )
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
        FilledTonalButton(onClick = { showLicenses = true }) {
            Icon(Icons.Outlined.Description, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.settings_licenses), modifier = Modifier.padding(start = 8.dp))
        }
        // parity wave A (worker q, checklist system.open-logs)
        TextButton(onClick = { showLogs = true }) {
            Icon(Icons.Outlined.Terminal, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.settings_logs), modifier = Modifier.padding(start = 8.dp))
        }
    }
    if (showLicenses) LicensesSheet(onDismiss = { showLicenses = false })
    if (showLogs) LogsSheet(system = system, onDismiss = { showLogs = false })
}

@Composable
private fun AboutRow(key: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(key, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = TatakaiFonts.jetBrainsMono), modifier = Modifier.weight(1.6f))
    }
}

/**
 * THIRD_PARTY_NOTICES.md (copied into the app assets at `licenses/THIRD_PARTY_NOTICES.md` by :app's build) followed by the OFL texts
 * of the bundled fonts (`licenses/OFL-*.txt`, shipped by :core:designsystem).
 */
@Composable
private fun LicensesSheet(onDismiss: () -> Unit) {
    val context = LocalContext.current
    val retryKey = rememberSaveable { mutableStateOf(0) }
    val notices by produceState<LoadState<List<String>>>(LoadState.Loading, retryKey.value) {
        value = withContext(Dispatchers.IO) { loadLicenses(context) }
    }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Text(
            stringResource(R.string.settings_licenses),
            style = MaterialTheme.typography.titleLarge,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        when (val n = notices) {
            LoadState.Loading -> if (rememberAfterDelay(150)) {
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { LoadingIndicator() }
            }
            is LoadState.Err -> ErrorState(
                message = n.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = { retryKey.value++ },
            )
            is LoadState.Ok -> if (n.value.isEmpty()) {
                EmptyState(icon = Icons.Outlined.Description, title = stringResource(R.string.settings_licenses_missing))
            } else {
                LazyColumn(
                    Modifier.fillMaxWidth().heightIn(max = 560.dp),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    items(n.value) { paragraph ->
                        Text(paragraph, style = MaterialTheme.typography.bodySmall.copy(fontFamily = TatakaiFonts.jetBrainsMono))
                    }
                }
            }
        }
    }
}

private fun loadLicenses(context: Context): LoadState<List<String>> = try {
    val assets = context.assets
    val files = buildList {
        if (assets.list("licenses")?.contains("THIRD_PARTY_NOTICES.md") == true) add("licenses/THIRD_PARTY_NOTICES.md")
        assets.list("licenses")?.filter { it.startsWith("OFL-") }?.sorted()?.forEach { add("licenses/$it") }
    }
    val paragraphs = files.flatMap { path ->
        val text = assets.open(path).bufferedReader().use { it.readText() }
        listOf("── ${path.substringAfterLast('/')} ──") + text.split(Regex("\\n\\s*\\n")).map(String::trim).filter(String::isNotEmpty)
    }
    LoadState.Ok(paragraphs)
} catch (e: java.io.IOException) {
    LoadState.Err(AppError.Unknown(e.message ?: "IOException"))
}
