// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.framegen

import androidx.annotation.StringRes

/**
 * 0.4.0 (worker B): string resources (es / en, `res/values{,-es}/strings_frame_gen.xml`) of the frame generation enums, shared by
 * Settings and the in-player UI so both say the same thing.
 */
object FrameGenText {
  /** Why FG cannot run ([FrameGenUiState.unavailable]); [noInputTimeout] picks the «no frames from MPV» variant. */
  @StringRes
  fun reason(reason: FgUnavailableReason, noInputTimeout: Boolean = false): Int = when (reason) {
    FgUnavailableReason.ABI -> R.string.fg_reason_abi
    FgUnavailableReason.LIBRARY -> R.string.fg_reason_library
    FgUnavailableReason.VULKAN -> R.string.fg_reason_vulkan
    FgUnavailableReason.SHADERS -> R.string.fg_reason_shaders
    FgUnavailableReason.PIP -> R.string.fg_reason_pip
    FgUnavailableReason.BACKGROUND -> R.string.fg_reason_background
    FgUnavailableReason.LOW_POWER -> R.string.fg_reason_low_power
    FgUnavailableReason.EIGHT_K -> R.string.fg_reason_eight_k
    FgUnavailableReason.HDR_PASSTHROUGH -> R.string.fg_reason_hdr_passthrough
    FgUnavailableReason.DEVICE_LOST -> R.string.fg_reason_device_lost
    FgUnavailableReason.START_FAILED -> if (noInputTimeout) R.string.fg_reason_no_input else R.string.fg_reason_start_failed
    FgUnavailableReason.REMOTE -> R.string.fg_reason_remote
  }

  @StringRes
  fun profile(profile: FgProfile): Int = when (profile) {
    FgProfile.SAVER -> R.string.fg_profile_saver
    FgProfile.BALANCED -> R.string.fg_profile_balanced
    FgProfile.QUALITY -> R.string.fg_profile_quality
    FgProfile.CUSTOM -> R.string.fg_profile_custom
  }

  /** Message of a Lossless.dll import result. */
  @StringRes
  fun install(result: InstallResult): Int = when (result) {
    InstallResult.IMPORTED -> R.string.fg_install_imported
    InstallResult.TOO_LARGE -> R.string.fg_install_too_large
    InstallResult.UNREADABLE -> R.string.fg_install_unreadable
    InstallResult.MISSING_SHADERS -> R.string.fg_install_missing_shaders
    InstallResult.WRITE_FAILED -> R.string.fg_install_write_failed
    InstallResult.UNAVAILABLE -> R.string.fg_install_unavailable
  }
}
