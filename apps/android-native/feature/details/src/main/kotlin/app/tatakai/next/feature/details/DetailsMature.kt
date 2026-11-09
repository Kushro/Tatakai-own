// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.MatureContentMode

/**
 * How Details presents an 18+ title according to `AppSettings.matureContent` (checklist settings.privacy.mature-content):
 * SHOW = as any title; BLUR = the page opens with banner and cover blurred until tapped; WARN = the classic AdultWarningGate, asked
 * once per title (remembered in DetailsPrefs).
 */
internal enum class AdultPresentation { NONE, BLUR, GATE }

internal fun adultPresentation(mode: MatureContentMode, adult: Boolean, accepted: Boolean): AdultPresentation = when {
    !adult -> AdultPresentation.NONE
    mode == MatureContentMode.SHOW -> AdultPresentation.NONE
    mode == MatureContentMode.BLUR -> AdultPresentation.BLUR
    accepted -> AdultPresentation.NONE
    else -> AdultPresentation.GATE
}
