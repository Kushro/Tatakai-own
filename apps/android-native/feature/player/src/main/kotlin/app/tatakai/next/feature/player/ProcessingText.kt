// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.annotation.StringRes
import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.LinkProfileGroupKind
import app.tatakai.next.model.ProcessingLocation

// 0.4.0 (worker C): the words of "Procesar en" for worker D's control and the notices. Resource ids only (no UI here).

@StringRes
fun processingLocationLabel(location: ProcessingLocation): Int = when (location) {
    ProcessingLocation.PHONE -> R.string.player_processing_phone
    ProcessingLocation.PC -> R.string.player_processing_pc
    ProcessingLocation.MIXED -> R.string.player_processing_mixed
}

/** One line under each option: what that location does. */
@StringRes
fun processingLocationHint(location: ProcessingLocation): Int = when (location) {
    ProcessingLocation.PHONE -> R.string.player_processing_phone_hint
    ProcessingLocation.PC -> R.string.player_processing_pc_hint
    ProcessingLocation.MIXED -> R.string.player_processing_mixed_hint
}

/** Why an option is disabled ([ProcessingOption.block]). */
@StringRes
fun processingBlockText(block: ProcessingBlock): Int = when (block) {
    ProcessingBlock.NOT_PAIRED -> R.string.player_processing_block_not_paired
    ProcessingBlock.PC_OFFLINE -> R.string.player_processing_block_offline
    ProcessingBlock.LOCAL_FILE -> R.string.player_processing_block_local_file
    ProcessingBlock.SOURCE_NOT_SENDABLE -> R.string.player_processing_block_not_sendable
    ProcessingBlock.NO_SOURCE_YET -> R.string.player_processing_block_no_source
    ProcessingBlock.EXTERNAL_PLAYER -> R.string.player_processing_block_external
    ProcessingBlock.NO_ORIGINAL_SOURCE -> R.string.player_processing_block_no_original
    ProcessingBlock.MPV_UNAVAILABLE -> R.string.player_processing_block_mpv
    ProcessingBlock.PHONE_FG_UNAVAILABLE -> R.string.player_processing_block_phone_fg
    ProcessingBlock.PC_SUPPRESSES_PHONE_FG -> R.string.player_processing_block_pc_fg
    ProcessingBlock.PC_NO_UPSCALE_PROFILE -> R.string.player_processing_block_no_upscale
}

@StringRes
fun processingFailureText(failure: ProcessingFailure): Int = when (failure) {
    ProcessingFailure.TIMEOUT -> R.string.player_processing_failed_timeout
    ProcessingFailure.OFFLINE -> R.string.player_processing_failed_offline
    ProcessingFailure.SOURCE_REFUSED -> R.string.player_processing_failed_source
    ProcessingFailure.PC_ERROR -> R.string.player_processing_failed_pc
    ProcessingFailure.LOCAL_UNAVAILABLE -> R.string.player_processing_failed_local
}

/** Headers of the in-player profile list ([ProcessingUiState.profileGroups]). */
@StringRes
fun linkProfileGroupLabel(kind: LinkProfileGroupKind): Int = when (kind) {
    LinkProfileGroupKind.NONE -> R.string.player_processing_group_none
    LinkProfileGroupKind.UPSCALE -> R.string.player_processing_group_upscale
    LinkProfileGroupKind.FRAME_GEN -> R.string.player_processing_group_frame_gen
    LinkProfileGroupKind.UPSCALE_FRAME_GEN -> R.string.player_processing_group_both
}

/** The flash text of a [PlayerNotice.Processing]. */
@Composable
internal fun processingNoticeText(notice: PlayerNotice.Processing): String {
    val reason = when {
        notice.block != null -> stringResource(processingBlockText(notice.block))
        notice.failure != null -> stringResource(processingFailureText(notice.failure))
        else -> null
    }
    return when {
        notice.onPhoneInstead -> stringResource(R.string.player_processing_on_phone_instead, reason.orEmpty())
        reason != null -> reason
        else -> stringResource(
            when (notice.to) {
                ProcessingLocation.PHONE -> R.string.player_processing_switched_phone
                ProcessingLocation.PC -> R.string.player_processing_switched_pc
                ProcessingLocation.MIXED -> R.string.player_processing_switched_mixed
            },
        )
    }
}
