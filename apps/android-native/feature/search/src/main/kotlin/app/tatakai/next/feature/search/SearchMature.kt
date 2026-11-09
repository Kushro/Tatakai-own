// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media

/**
 * Checklist settings.privacy.mature-content in Search: with `matureContent = BLUR` an adult result keeps its cover blurred until the
 * user taps it once ([revealed] holds the `mediaKey`s already uncovered on this screen); the next tap opens Details. SHOW opens as any
 * title; WARN opens Details, whose AdultGate asks once per title.
 */
internal fun searchCoverBlurred(mode: MatureContentMode, media: Media, key: String, revealed: Set<String>): Boolean =
    mode == MatureContentMode.BLUR && media.isProbablyAdult() && key !in revealed
