// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.4, 2026-11-09 amendment, checklist
 * account.setup-wizard / account.onboarding). Owner: worker q.
 *
 * First-run wizard (classic onboarding + setup wizard, trimmed to what Next has): welcome → language and theme → connect AniList / MAL
 * (optional, Custom Tab through `ListsRepository.beginLink`) → link a PC (optional, opens the pairing screen later) → done. Every step can
 * be skipped (`settings_onboarding_skip`). The last step (or Skip) writes `AppSettings.onboardingDone = true` through
 * `SettingsRepository` (a small `OnboardingViewModel`, private to this module, registered in `settingsModule`), and the shell reacts to
 * the flag; [onDone] is an extra hook for the shell and may be a no-op. Shown by `TatakaiApp` while `onboardingDone` is false, so it
 * runs again after "Restablecer la app".
 */
@Composable
fun OnboardingScreen(onDone: () -> Unit, modifier: Modifier = Modifier) {
    Text("OnboardingScreen", modifier)
}
