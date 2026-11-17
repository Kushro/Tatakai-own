// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import android.content.res.Resources
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.AutoAwesome
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Computer
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Restore
import androidx.compose.material.icons.rounded.WarningAmber
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.SIDE_SHEET_MIN_WIDTH_DP
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.feature.link.GroupedProfileList
import app.tatakai.next.feature.link.ProfileList
import app.tatakai.next.feature.link.linkDegradedText
import app.tatakai.next.feature.link.linkEncoderText
import app.tatakai.next.feature.link.linkFrameGenText
import app.tatakai.next.feature.link.linkUpscaleText
import app.tatakai.next.feature.link.profileLabel
import app.tatakai.next.framegen.FgProfile
import app.tatakai.next.framegen.FgStatsStatus
import app.tatakai.next.framegen.FgStatsView
import app.tatakai.next.framegen.FgUnavailableReason
import app.tatakai.next.framegen.FrameGenController
import app.tatakai.next.framegen.FrameGenMultiplier
import app.tatakai.next.framegen.FrameGenText
import app.tatakai.next.framegen.FrameGenUiState
import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.model.LinkUpscale
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityUiState
import app.tatakai.next.player.decoderInUseLabel
import java.util.Locale
import kotlin.math.roundToInt
import app.tatakai.next.framegen.R as FgR
import app.tatakai.next.player.R as QualityR

// 0.4.0 (worker D, artifacts/player-quality-0.4.0-specs/d-player-ui.md): the in-player "Mejoras" panel and its status chip.
// Adapted from Enigmata's QualitySheet.kt ("Procesar en: Teléfono | PC | Mixto" on top, then engine / tier / presets / sharpen /
// status reasons), ImagenContent.kt (quick FG card, zoom, aspect, decoder, image filters) and ProcessedOnPcPanel.kt ("what the PC
// does"), AGPL-3.0-or-later, same author, redrawn with Tatakai's design system. Every change applies to the running playback (the
// engines do it live); only "Usar como predeterminado" writes Settings.

// ---- words ---------------------------------------------------------------------------------------------------------------------------

@StringRes
internal fun presetLabelRes(id: String): Int = when (id) {
    QualityPresets.ANIME4K_A -> R.string.player_fx_preset_anime4k_a
    QualityPresets.ANIME4K_AA -> R.string.player_fx_preset_anime4k_aa
    QualityPresets.ANIME4K_B -> R.string.player_fx_preset_anime4k_b
    QualityPresets.ANIME4K_C -> R.string.player_fx_preset_anime4k_c
    QualityPresets.ANIME4K_BB -> R.string.player_fx_preset_anime4k_bb
    QualityPresets.ANIME4K_CA -> R.string.player_fx_preset_anime4k_ca
    QualityPresets.FSR -> R.string.player_fx_preset_fsr
    QualityPresets.FIDELITY -> R.string.player_fx_preset_fidelity
    QualityPresets.CAS -> R.string.player_fx_preset_cas
    QualityPresets.ARTCNN -> R.string.player_fx_preset_artcnn
    else -> R.string.player_fx_preset_off
}

@StringRes
private fun presetHintRes(id: String): Int? = when (id) {
    QualityPresets.OFF -> R.string.player_fx_hint_off
    QualityPresets.ANIME4K_A -> R.string.player_fx_hint_anime4k_a
    QualityPresets.ANIME4K_AA -> R.string.player_fx_hint_anime4k_aa
    QualityPresets.ANIME4K_B -> R.string.player_fx_hint_anime4k_b
    QualityPresets.ANIME4K_C -> R.string.player_fx_hint_anime4k_c
    QualityPresets.ANIME4K_BB -> R.string.player_fx_hint_anime4k_bb
    QualityPresets.ANIME4K_CA -> R.string.player_fx_hint_anime4k_ca
    QualityPresets.FSR -> R.string.player_fx_hint_fsr
    QualityPresets.FIDELITY -> R.string.player_fx_hint_fidelity
    QualityPresets.CAS -> R.string.player_fx_hint_cas
    else -> null
}

@StringRes
internal fun tierLabelRes(tier: QualityTier): Int = when (tier) {
    QualityTier.FAST -> R.string.player_fx_tier_fast
    QualityTier.BALANCED -> R.string.player_fx_tier_balanced
    QualityTier.HQ -> R.string.player_fx_tier_hq
}

/** [ChipWords] from this module's resources (es / en). */
private class ResourceChipWords(private val res: Resources) : ChipWords {
    override fun preset(id: String) = res.getString(presetLabelRes(id))
    override fun tier(tier: QualityTier) = res.getString(tierLabelRes(tier))
    override val noEnhancements: String get() = res.getString(R.string.player_fx_none)
    override val restricted: String get() = res.getString(R.string.player_fx_restricted)
    override val paused: String get() = res.getString(R.string.player_fx_paused)
    override fun frameGen(multiplier: Int) = res.getString(R.string.player_fx_fg, multiplier)
    override val deband: String get() = res.getString(R.string.player_fx_deband)
    override val sharpen: String get() = res.getString(R.string.player_fx_sharpen)
    override val smoothing: String get() = res.getString(R.string.player_fx_smoothing)
    override val media3: String get() = res.getString(R.string.player_fx_media3)
    override val pc: String get() = res.getString(R.string.player_fx_pc)
    override val pcNothing: String get() = res.getString(R.string.player_fx_pc_nothing)
    override fun pcAnime4k(height: Int?) =
        height?.let { res.getString(R.string.player_fx_pc_anime4k, it) } ?: res.getString(R.string.player_fx_pc_anime4k_plain)
    override fun pcLsfg(multiplier: Int?) =
        multiplier?.let { res.getString(R.string.player_fx_pc_lsfg, it) } ?: res.getString(R.string.player_fx_pc_lsfg_plain)
    override val pcLsfgAuto: String get() = res.getString(R.string.player_fx_pc_lsfg_auto)
    override val pcFrc: String get() = res.getString(R.string.player_fx_pc_frc)
    override val mixedPcScales: String get() = res.getString(R.string.player_fx_mixed_pc)
    override fun mixedPhoneFg(multiplier: Int?) =
        multiplier?.let { res.getString(R.string.player_fx_mixed_fg, it) } ?: res.getString(R.string.player_fx_mixed_fg_plain)
    override fun switching(to: ProcessingLocation) = res.getString(
        when (to) {
            ProcessingLocation.PHONE -> R.string.player_fx_switching_phone
            ProcessingLocation.PC -> R.string.player_fx_switching_pc
            ProcessingLocation.MIXED -> R.string.player_fx_switching_mixed
        },
    )
}

@Composable
internal fun rememberChipWords(): ChipWords {
    val res = LocalContext.current.resources
    return remember(res, LocalConfiguration.current) { ResourceChipWords(res) }
}

/** "Anime4K A+A · HQ" of a level (no FG, no status), or "Sin escalado". */
@Composable
internal fun levelText(level: QualityLevel): String =
    if (level.off) stringResource(R.string.player_fx_preset_off)
    else listOfNotNull(stringResource(presetLabelRes(level.preset)), level.tier?.let { stringResource(tierLabelRes(it)) }).joinToString(CHIP_SEPARATOR)

// ---- status chip -----------------------------------------------------------------------------------------------------------------------

/** The top bar's status chip: what runs and where; a tap opens the panel. */
@Composable
internal fun EnhancementStatusChip(chip: EnhancementChip, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val label = chipLabel(chip, rememberChipWords())
    val tone = chipTone(chip)
    val scheme = MaterialTheme.colorScheme
    val on = scheme.onBackground
    val (container, content) = when (tone) {
        ChipTone.ACTIVE -> scheme.primaryContainer to scheme.onPrimaryContainer
        ChipTone.WARNING -> scheme.errorContainer to scheme.onErrorContainer
        ChipTone.NEUTRAL -> Color.Transparent to on
    }
    val description = stringResource(R.string.player_fx_chip_desc, label)
    AssistChip(
        onClick = onClick,
        label = { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingIcon = {
            when {
                chip is EnhancementChip.Switching -> CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp, color = content)
                tone == ChipTone.WARNING -> Icon(Icons.Rounded.WarningAmber, null, Modifier.size(18.dp))
                chip is EnhancementChip.Pc -> Icon(Icons.Rounded.Computer, null, Modifier.size(18.dp))
                chip is EnhancementChip.Mixed -> Icon(Icons.Rounded.Devices, null, Modifier.size(18.dp))
                else -> Icon(Icons.Rounded.AutoAwesome, null, Modifier.size(18.dp))
            }
        },
        colors = AssistChipDefaults.assistChipColors(containerColor = container, labelColor = content, leadingIconContentColor = content),
        border = if (tone == ChipTone.NEUTRAL) AssistChipDefaults.assistChipBorder(enabled = true) else null,
        modifier = modifier.semantics { contentDescription = description },
    )
}

// ---- container ---------------------------------------------------------------------------------------------------------------------------

/**
 * Side sheet on wide windows (tablet, phone in landscape: 400 dp, the video stays visible beside it), bottom sheet on a phone in
 * portrait. [AdaptiveSheet] scrolls its content itself on the wide branch; the bottom sheet branch scrolls here.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun EnhancementSheet(onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    val wide = LocalConfiguration.current.screenWidthDp >= SIDE_SHEET_MIN_WIDTH_DP
    if (wide) {
        AdaptiveSheet(onDismissRequest = onDismiss, content = content)
    } else {
        ModalBottomSheet(onDismissRequest = onDismiss, sheetState = rememberModalBottomSheetState()) {
            Column(Modifier.verticalScroll(rememberScrollState()).navigationBarsPadding().padding(bottom = 16.dp), content = content)
        }
    }
}

// ---- panel -------------------------------------------------------------------------------------------------------------------------------

/**
 * The "Mejoras" panel. Sections: a. Procesar en (Teléfono | PC | Mixto), b. what the PC does + its profiles (PC / Mixto),
 * c. upscaling on the phone (libmpv) or "Las mejoras necesitan MPV" (Media3), d. the phone's frame generation, e. picture
 * (aspect, zoom, filters, decoder), f. statistics overlay switch.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun EnhancementPanel(
    vm: PlayerViewModel,
    session: PlayerSession,
    state: PlayerUiState,
    quality: QualityUiState,
    processing: ProcessingUiState,
    fg: FrameGenController?,
    fgState: FrameGenUiState?,
    statsVisible: Boolean,
    onStatsChange: (Boolean) -> Unit,
    onOpenFgSettings: () -> Unit,
    onClose: () -> Unit,
) {
    val location = processing.location
    val mpv = quality.available
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.player_fx_title),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f).semantics { heading() },
            )
            IconButton(onClick = onClose) { Icon(Icons.Rounded.Close, stringResource(R.string.player_fx_close)) }
        }

        ProcessingSection(vm, processing, onOpenFgSettings)

        if (location != ProcessingLocation.PHONE) {
            PcSection(vm, state, processing)
        }
        when {
            location == ProcessingLocation.PHONE && !mpv -> Media3Section(vm, onClose)
            location == ProcessingLocation.PHONE -> PhoneQualitySection(vm, session, state, quality)
            location == ProcessingLocation.MIXED -> Note(stringResource(R.string.player_fx_mixed_note))
            else -> Note(stringResource(R.string.player_processing_local_off))
        }
        // FG only exists on libmpv; strict PC suspends it (the PC already processes everything).
        if (fg != null && fgState != null && mpv && location != ProcessingLocation.PC) {
            FrameGenSection(fg, fgState, onOpenFgSettings)
        }
        if (mpv) PictureSection(session, quality)

        SectionCard {
            SwitchRow(
                title = stringResource(R.string.player_fx_stats),
                hint = stringResource(R.string.player_fx_stats_hint),
                checked = statsVisible,
                onChange = onStatsChange,
            )
        }
        Spacer(Modifier.size(8.dp))
    }
}

// a. Procesar en ----------------------------------------------------------------------------------------------------------------------------

@Composable
private fun ProcessingSection(vm: PlayerViewModel, processing: ProcessingUiState, onOpenFgSettings: () -> Unit) {
    val selected = processing.switchingTo ?: processing.location
    SectionCard(title = stringResource(R.string.player_processing_title)) {
        Segmented(
            options = ProcessingLocation.entries.map { loc ->
                val option = processing.option(loc)
                SegmentedOption(
                    value = loc,
                    label = stringResource(shortLocationRes(loc)),
                    enabled = processing.switchingTo == null && (option.available || loc == processing.location),
                    description = stringResource(processingLocationHint(loc)),
                )
            },
            selected = selected,
            onSelect = vm::setProcessingLocation,
            fillWidth = true,
        )
        processing.switchingTo?.let { to ->
            Row(Modifier.fillMaxWidth().padding(top = 8.dp), Arrangement.spacedBy(12.dp), Alignment.CenterVertically) {
                CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
                Text(chipLabel(EnhancementChip.Switching(to), rememberChipWords()), Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                TextButton(onClick = vm::cancelProcessingSwitch) { Text(stringResource(R.string.player_fx_switch_cancel)) }
            }
        }
        // What each location does, in one line, and why it cannot be chosen now (Enigmata's QualitySheet does the same).
        var fgShaders = false
        ProcessingLocation.entries.forEach { loc ->
            val option = processing.option(loc)
            val why = option.block?.takeIf { loc != processing.location }?.let { block ->
                val fgReason = option.detail?.let { d -> FgUnavailableReason.entries.firstOrNull { it.name.equals(d, ignoreCase = true) } }
                    ?.takeIf { block == ProcessingBlock.PHONE_FG_UNAVAILABLE }
                if (fgReason == FgUnavailableReason.SHADERS) fgShaders = true
                stringResource(processingBlockText(block)) + (fgReason?.let { " " + stringResource(FrameGenText.reason(it)) } ?: "")
            }
            Column(Modifier.fillMaxWidth().padding(top = 10.dp)) {
                Text(
                    stringResource(processingLocationLabel(loc)),
                    style = MaterialTheme.typography.labelLarge,
                    color = if (loc == selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                )
                Text(stringResource(processingLocationHint(loc)), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                why?.let { InfoLine(it) }
            }
        }
        if (fgShaders) {
            TextButton(onClick = onOpenFgSettings) { Text(stringResource(R.string.player_fx_fg_import)) }
        }
    }
}

@StringRes
private fun shortLocationRes(location: ProcessingLocation): Int = when (location) {
    ProcessingLocation.PHONE -> R.string.player_fx_where_phone
    ProcessingLocation.PC -> R.string.player_fx_where_pc
    ProcessingLocation.MIXED -> R.string.player_fx_where_mixed
}

// b. What the PC does ---------------------------------------------------------------------------------------------------------------------

@Composable
private fun PcSection(vm: PlayerViewModel, state: PlayerUiState, processing: ProcessingUiState) {
    val info = processing.pc
    val name = info?.pcName ?: processing.pcName
    SectionCard(title = name?.let { stringResource(R.string.player_fx_pc_card_named, it) } ?: stringResource(R.string.player_fx_pc_card)) {
        if (info == null) {
            InfoLine(stringResource(R.string.player_fx_pc_waiting))
        } else {
            PcRows(info, state)
        }
    }
    val groups = processing.profileGroups
    if (groups.isNotEmpty() || state.linkProfiles.isNotEmpty()) {
        SectionCard(title = stringResource(R.string.player_link_profile)) {
            Text(stringResource(R.string.player_link_profile_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            if (processing.profileChanging || state.linkProfileChanging) {
                Row(Modifier.padding(top = 8.dp), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
                    CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                    Text(stringResource(R.string.player_link_profile_changing), style = MaterialTheme.typography.bodyMedium)
                }
            }
            val selected = processing.profile ?: state.linkProfile.orEmpty()
            if (groups.isNotEmpty()) {
                // grouped and filtered like the Linked screen ("Solo escalado" only in Mixto)
                GroupedProfileList(groups = groups, selected = selected, groupLabel = { stringResource(linkProfileGroupLabel(it)) }) { id ->
                    vm.setLinkProfile(id)
                }
            } else {
                ProfileList(state.linkProfiles, selected = selected, unavailable = state.linkUnsupported, mayDropFrames = state.linkMayDropFrames) { id ->
                    vm.changeLinkProfile(id)
                }
            }
        }
    }
}

@Composable
private fun PcRows(info: LinkProcessingInfo, state: PlayerUiState) {
    val profiles = state.linkProfiles
    val profile = profiles.firstOrNull { it.id == info.profileId }?.let { profileLabel(it) } ?: info.profileLabel ?: info.profileId
    InfoRow(stringResource(R.string.player_fx_pc_row_profile), profile)
    val upscale = linkUpscaleText(info) ?: when (info.upscale) {
        is LinkUpscale.Anime4k -> stringResource(R.string.player_fx_pc_anime4k_plain)
        LinkUpscale.None -> stringResource(R.string.player_fx_pc_no_upscale)
    }
    InfoRow(stringResource(R.string.player_fx_pc_row_upscale), upscale)
    InfoRow(stringResource(R.string.player_fx_pc_row_fg), linkFrameGenText(info))
    linkEncoderText(info)?.let { InfoRow(stringResource(R.string.player_fx_pc_row_codec), it) }
    info.leadSecs?.let { lead ->
        val secs = lead.roundToInt().coerceAtLeast(0)
        InfoRow(
            stringResource(R.string.player_fx_pc_row_lead),
            stringResource(if (info.throttled) R.string.player_fx_pc_throttled else R.string.player_fx_pc_lead_value, secs),
        )
    }
    if (info.fallingBehind) WarningLine(stringResource(R.string.player_processing_pc_falling_behind))
    linkDegradedText(info) { id -> profiles.firstOrNull { it.id == id }?.let { profileLabel(it) } ?: id }?.let { WarningLine(it) }
}

// c. Upscaling on the phone / Media3 ----------------------------------------------------------------------------------------------------

@Composable
private fun Media3Section(vm: PlayerViewModel, onClose: () -> Unit) {
    SectionCard(title = stringResource(R.string.player_fx_media3_title)) {
        Text(stringResource(R.string.player_fx_media3_body), style = MaterialTheme.typography.bodyMedium)
        if (vm.canSwitchToMpvNow()) {
            Button(onClick = { if (vm.switchToMpvByUser()) onClose() }, modifier = Modifier.padding(top = 12.dp)) {
                Text(stringResource(QualityR.string.quality_switch_to_mpv))
            }
        } else {
            InfoLine(stringResource(R.string.player_fx_media3_cannot))
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PhoneQualitySection(vm: PlayerViewModel, session: PlayerSession, state: PlayerUiState, q: QualityUiState) {
    val req = q.requested
    SectionCard(title = stringResource(R.string.player_fx_phone_section)) {
        // What runs now and why it differs from the choice
        Text(stringResource(R.string.player_fx_now, levelText(q.effective)), style = MaterialTheme.typography.bodyLarge)
        q.reasons.forEach { r ->
            if (r in RESTRICTING_REASONS) WarningLine(stringResource(r.labelRes)) else InfoLine(stringResource(r.labelRes))
        }
        q.autoDowngrade?.let { d ->
            Row(Modifier.fillMaxWidth().padding(top = 4.dp), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
                Text(
                    stringResource(d.reason.labelRes) + " (" + levelText(d.from) + ")",
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.weight(1f),
                )
                FilledTonalButton(onClick = session::undoAutoDowngrade) {
                    Icon(Icons.Rounded.Restore, null, Modifier.size(18.dp))
                    Spacer(Modifier.width(6.dp))
                    Text(stringResource(QualityR.string.quality_undo))
                }
            }
        }

        // Presets
        FlowRow(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            q.presets.forEach { p ->
                FilterChip(
                    selected = req.preset == p.id,
                    onClick = { session.setQuality(p.id, req.tier) },
                    enabled = p.available,
                    label = { Text(stringResource(presetLabelRes(p.id))) },
                )
            }
        }
        presetHintRes(req.preset)?.let { InfoLine(stringResource(it)) }
        q.presets.filter { !it.available }.forEach { p ->
            p.unavailableReason?.let { InfoLine(stringResource(R.string.player_fx_blocked_line, stringResource(presetLabelRes(p.id)), stringResource(it.labelRes))) }
        }

        // Tier
        LabelText(stringResource(R.string.player_fx_tier))
        val tiered = req.preset in QualityPresets.TIERED
        Segmented(
            options = QualityTier.entries.map { SegmentedOption(it, stringResource(tierLabelRes(it)), enabled = tiered) },
            selected = req.tier,
            onSelect = { session.setQuality(req.preset, it) },
            fillWidth = true,
            small = true, // "Equilibrado" fits a phone's third of the width
        )
        InfoLine(stringResource(R.string.player_fx_tier_hint))

        // Persisting: the player's changes are for this playback; "Usar como predeterminado" writes Settings
        val isDefault = QualityPresets.sanitize(state.settings.playerPreset) == req.preset && state.settings.playerQuality.tier == req.tier
        Row(Modifier.fillMaxWidth(), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
            Text(
                stringResource(R.string.player_fx_session_only),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { vm.saveQualityAsDefault(req.preset, req.tier) }, enabled = !isDefault) {
                Text(stringResource(if (isDefault) R.string.player_fx_is_default else R.string.player_fx_save_default))
            }
        }

        HorizontalDivider(Modifier.padding(vertical = 8.dp))
        LabelText(stringResource(R.string.player_fx_sharpen))
        Segmented(
            options = SharpenLevel.entries.map { SegmentedOption(it, stringResource(sharpenRes(it))) },
            selected = req.sharpen,
            onSelect = session::setSharpen,
            fillWidth = true,
        )
        SwitchRow(
            title = stringResource(R.string.player_fx_deband),
            hint = stringResource(R.string.player_fx_deband_hint),
            checked = req.deband.enabled,
            onChange = { session.setDeband(req.deband.copy(enabled = it)) },
        )
        SwitchRow(
            title = stringResource(R.string.player_fx_smoothing_title),
            hint = if (req.smoothing && q.frameGenActive) stringResource(QualityR.string.quality_reason_frame_gen) else stringResource(R.string.player_fx_smoothing_hint),
            checked = req.smoothing,
            onChange = session::setSmoothing,
        )
        SwitchRow(
            title = stringResource(R.string.player_fx_auto),
            hint = stringResource(R.string.player_fx_auto_hint),
            checked = req.autoAdjust,
            onChange = session::setAutoAdjust,
        )
    }
}

@StringRes
private fun sharpenRes(level: SharpenLevel): Int = when (level) {
    SharpenLevel.OFF -> R.string.player_fx_sharpen_off
    SharpenLevel.LOW -> R.string.player_fx_sharpen_low
    SharpenLevel.MEDIUM -> R.string.player_fx_sharpen_medium
    SharpenLevel.HIGH -> R.string.player_fx_sharpen_high
}

// d. Phone frame generation -------------------------------------------------------------------------------------------------------------

@Composable
private fun FrameGenSection(fg: FrameGenController, s: FrameGenUiState, onOpenFgSettings: () -> Unit) {
    // the expensive eligibility facts (library, imported shaders) are probed when the card is shown
    LaunchedEffect(fg) { fg.refresh() }
    val reason = s.unavailable
    val ineligible = reason?.kind == FgUnavailableReason.Kind.INELIGIBLE
    SectionCard(title = stringResource(R.string.player_fx_fg_title)) {
        val status = when {
            s.active -> stringResource(FgR.string.fg_state_active, s.multiplier)
            !s.eligibilityKnown && reason == null -> stringResource(FgR.string.fg_state_checking)
            reason != null -> stringResource(FrameGenText.reason(reason, s.noInputTimeout))
            s.enabled -> stringResource(FgR.string.fg_state_ready)
            else -> stringResource(FgR.string.fg_state_off)
        }
        SwitchRow(
            title = stringResource(FgR.string.fg_enabled),
            hint = status,
            checked = s.enabled,
            enabled = !ineligible || s.enabled,
            onChange = fg::setEnabled,
            warning = reason != null && reason.kind != FgUnavailableReason.Kind.SUSPENDED,
        )
        if (reason == FgUnavailableReason.SHADERS || s.shadersImported == false) {
            TextButton(onClick = onOpenFgSettings) { Text(stringResource(R.string.player_fx_fg_import)) }
        }
        if (s.active) {
            val view = FgStatsView.from(s, s.liveFps)
            if (view.status == FgStatsStatus.ACTIVE && view.inFps > 0.0) {
                Text(
                    stringResource(R.string.player_fx_fg_live, fpsText(view.inFps), fpsText(view.outFps), view.multiplier),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.primary,
                )
            }
            if (view.overloaded) WarningLine(stringResource(FgR.string.fg_state_overloaded))
        }
        if (s.experimental) InfoLine(stringResource(FgR.string.fg_state_experimental))
        if (!ineligible) {
            LabelText(stringResource(FgR.string.fg_multiplier))
            val selectedMultiplier = if (s.auto) FrameGenMultiplier.AUTO else FrameGenMultiplier.of(s.multiplier)
            Segmented(
                options = FrameGenMultiplier.entries.map { m ->
                    SegmentedOption(m, if (m == FrameGenMultiplier.AUTO) stringResource(FgR.string.fg_multiplier_auto) else stringResource(FgR.string.fg_multiplier_n, m.n))
                },
                selected = selectedMultiplier,
                onSelect = fg::setMultiplier,
                fillWidth = true,
                small = true,
            )
            InfoLine(stringResource(FgR.string.fg_multiplier_hint))
            LabelText(stringResource(FgR.string.fg_profile))
            // CUSTOM is set up in Settings; it only shows here while it is the one in use
            val profiles = listOf(FgProfile.SAVER, FgProfile.BALANCED, FgProfile.QUALITY) + listOfNotNull(FgProfile.CUSTOM.takeIf { s.profile == it })
            Segmented(
                options = profiles.map { SegmentedOption(it, stringResource(FrameGenText.profile(it))) },
                selected = s.profile,
                onSelect = fg::setProfile,
                fillWidth = true,
                small = true,
            )
            InfoLine(stringResource(FgR.string.fg_profile_hint))
        }
    }
}

internal fun fpsText(fps: Double): String =
    if (fps % 1.0 == 0.0) fps.toInt().toString() else String.format(Locale.ROOT, "%.1f", fps)

// e. Picture --------------------------------------------------------------------------------------------------------------------------

@Composable
private fun PictureSection(session: PlayerSession, q: QualityUiState) {
    val req = q.requested
    SectionCard(title = stringResource(R.string.player_fx_image)) {
        LabelText(stringResource(R.string.player_fx_aspect))
        Segmented(
            options = AspectMode.entries.map { SegmentedOption(it, stringResource(aspectRes(it))) },
            selected = req.aspect,
            onSelect = { session.setAspect(it, req.zoomPercent) },
            small = true,
        )
        SliderRow(
            label = stringResource(R.string.player_fx_zoom),
            value = req.zoomPercent,
            range = app.tatakai.next.player.QualityRequest.MIN_ZOOM..app.tatakai.next.player.QualityRequest.MAX_ZOOM,
            step = 5,
            valueText = stringResource(R.string.player_fx_percent, req.zoomPercent),
            onChange = { session.setAspect(req.aspect, it) },
        )
        val img = req.image
        listOf(
            Triple(R.string.player_fx_brightness, img.brightness) { v: Int -> img.copy(brightness = v) },
            Triple(R.string.player_fx_contrast, img.contrast) { v: Int -> img.copy(contrast = v) },
            Triple(R.string.player_fx_saturation, img.saturation) { v: Int -> img.copy(saturation = v) },
            Triple(R.string.player_fx_gamma, img.gamma) { v: Int -> img.copy(gamma = v) },
            Triple(R.string.player_fx_hue, img.hue) { v: Int -> img.copy(hue = v) },
        ).forEach { (label, value, change) ->
            SliderRow(
                label = stringResource(label),
                value = value,
                range = ImageAdjust.MIN..ImageAdjust.MAX,
                step = 1,
                valueText = if (value > 0) "+$value" else "$value",
                onChange = { session.setImage(change(it)) },
            )
        }
        if (!img.isNeutral || req.zoomPercent != 100 || req.aspect != AspectMode.FIT) {
            TextButton(onClick = { session.setImage(ImageAdjust()); session.setAspect(AspectMode.FIT, 100) }) {
                Icon(Icons.Rounded.Restore, null, Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(stringResource(R.string.player_fx_reset))
            }
        }
        HorizontalDivider(Modifier.padding(vertical = 8.dp))
        LabelText(stringResource(R.string.player_fx_decoder))
        Segmented(
            options = DecoderMode.entries.map { SegmentedOption(it, stringResource(decoderRes(it))) },
            selected = req.decoder,
            onSelect = session::setDecoder,
            fillWidth = true,
        )
        decoderInUseLabel(q.decoderInUse)?.let { InfoLine(stringResource(R.string.player_fx_decoder_in_use, it)) }
        InfoLine(stringResource(R.string.player_fx_decoder_hint))
    }
}

@StringRes
private fun aspectRes(mode: AspectMode): Int = when (mode) {
    AspectMode.FIT -> R.string.player_fx_aspect_fit
    AspectMode.FILL -> R.string.player_fx_aspect_fill
    AspectMode.STRETCH -> R.string.player_fx_aspect_stretch
    AspectMode.RATIO_16_9 -> R.string.player_fx_aspect_16_9
    AspectMode.RATIO_4_3 -> R.string.player_fx_aspect_4_3
}

@StringRes
internal fun decoderRes(mode: DecoderMode): Int = when (mode) {
    DecoderMode.AUTO -> R.string.player_fx_decoder_auto
    DecoderMode.HW -> R.string.player_fx_decoder_hw
    DecoderMode.HW_PLUS -> R.string.player_fx_decoder_hw_plus
    DecoderMode.SW -> R.string.player_fx_decoder_sw
}

// ---- small pieces ----------------------------------------------------------------------------------------------------------------------

@Composable
private fun LabelText(text: String) {
    Text(text, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 12.dp, bottom = 6.dp))
}

@Composable
private fun InfoLine(text: String) {
    Row(Modifier.padding(top = 6.dp), Arrangement.spacedBy(6.dp)) {
        Icon(Icons.Rounded.Info, null, Modifier.size(16.dp).padding(top = 2.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun WarningLine(text: String) {
    Row(Modifier.padding(top = 6.dp), Arrangement.spacedBy(6.dp)) {
        Icon(Icons.Rounded.WarningAmber, null, Modifier.size(16.dp).padding(top = 2.dp), tint = MaterialTheme.colorScheme.error)
        Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
    }
}

@Composable
private fun Note(text: String) {
    SectionCard { InfoLine(text) }
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), Arrangement.spacedBy(12.dp)) {
        Text(label, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(0.45f))
        Text(value, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(0.55f))
    }
}

/** A setting with a switch; the whole row is the touch target. */
@Composable
private fun SwitchRow(
    title: String,
    hint: String?,
    checked: Boolean,
    onChange: (Boolean) -> Unit,
    enabled: Boolean = true,
    warning: Boolean = false,
) {
    Row(Modifier.fillMaxWidth().padding(top = 8.dp), Arrangement.spacedBy(12.dp), Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            hint?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = if (warning) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Switch(checked = checked, onCheckedChange = onChange, enabled = enabled)
    }
}

/** An integer slider ([step] apart) applied while dragging: mpv takes these properties live and cheaply. */
@Composable
private fun SliderRow(label: String, value: Int, range: IntRange, step: Int, valueText: String, onChange: (Int) -> Unit) {
    Column(Modifier.fillMaxWidth().padding(top = 4.dp)) {
        Row(Modifier.fillMaxWidth()) {
            Text(label, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            Text(valueText, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
        }
        Slider(
            value = value.toFloat(),
            onValueChange = { v ->
                val snapped = ((v / step).roundToInt() * step).coerceIn(range.first, range.last)
                if (snapped != value) onChange(snapped)
            },
            valueRange = range.first.toFloat()..range.last.toFloat(),
            modifier = Modifier.semantics { contentDescription = "$label $valueText" },
        )
    }
}
