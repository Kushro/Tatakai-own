// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.settings

import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.Celebration
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Palette
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.ThemeMode
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel

/** Steps of the first-run wizard, in order. */
internal enum class OnboardingStep { WELCOME, APPEARANCE, LISTS, LINK, DONE }

/** First-run wizard state: writes language / theme at once and `onboardingDone = true` at the end (or on Skip). */
class OnboardingViewModel(private val settingsRepo: SettingsRepository) : ViewModel() {
    val settings: StateFlow<AppSettings> = settingsRepo.settings

    fun setLanguage(value: AppLanguage) = viewModelScope.launch { settingsRepo.update { it.copy(language = value) } }

    fun setThemeMode(value: ThemeMode) = viewModelScope.launch { settingsRepo.update { it.copy(themeMode = value) } }

    fun finish(onDone: () -> Unit) {
        viewModelScope.launch {
            settingsRepo.update { it.copy(onboardingDone = true) }
            onDone()
        }
    }
}

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.4, 2026-11-09 amendment, checklist account.setup-wizard).
 * Owner: worker q.
 *
 * First-run wizard (classic onboarding + setup wizard, trimmed to what Next has): welcome → language and theme → connect AniList / MAL
 * (optional, Custom Tab through the same [AccountsViewModel] as Settings) → link a PC (optional; explains where pairing lives) → done.
 * Every step can be skipped. The last step (or Skip) writes `AppSettings.onboardingDone = true`; the shell reacts to the flag and
 * [onDone] is an extra hook. Shown by `TatakaiApp` while `onboardingDone` is false, so it runs again after "Restablecer la app".
 */
@Composable
fun OnboardingScreen(onDone: () -> Unit, modifier: Modifier = Modifier) {
    val vm: OnboardingViewModel = koinViewModel()
    val settings by vm.settings.collectAsStateWithLifecycle()
    var stepIndex by rememberSaveable { mutableIntStateOf(0) }
    val step = OnboardingStep.entries[stepIndex.coerceIn(0, OnboardingStep.entries.lastIndex)]
    val finish = { vm.finish(onDone) }

    Surface(modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
        Box(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.fillMaxSize().widthIn(max = 560.dp).padding(horizontal = 24.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    LinearProgressIndicator(
                        progress = { (stepIndex + 1f) / OnboardingStep.entries.size },
                        modifier = Modifier.weight(1f),
                    )
                    if (step != OnboardingStep.DONE) {
                        TextButton(onClick = finish, modifier = Modifier.padding(start = 8.dp)) {
                            Text(stringResource(R.string.settings_onboarding_skip))
                        }
                    }
                }
                AnimatedContent(targetState = step, modifier = Modifier.weight(1f), label = "onboarding") { current ->
                    Column(
                        Modifier.fillMaxSize().verticalScroll(rememberScrollState()),
                        verticalArrangement = Arrangement.spacedBy(16.dp),
                    ) {
                        when (current) {
                            OnboardingStep.WELCOME -> StepHeader(
                                Icons.Outlined.Celebration,
                                stringResource(R.string.settings_onboarding_title),
                                stringResource(R.string.settings_onboarding_welcome_body),
                            )
                            OnboardingStep.APPEARANCE -> {
                                StepHeader(
                                    Icons.Outlined.Palette,
                                    stringResource(R.string.settings_onboarding_appearance_title),
                                    stringResource(R.string.settings_onboarding_appearance_body),
                                )
                                Text(stringResource(R.string.settings_language), style = MaterialTheme.typography.labelLarge)
                                Segmented(
                                    options = listOf(
                                        SegmentedOption(AppLanguage.SYSTEM, stringResource(R.string.settings_lang_system)),
                                        SegmentedOption(AppLanguage.EN, stringResource(R.string.settings_lang_en)),
                                        SegmentedOption(AppLanguage.ES, stringResource(R.string.settings_lang_es)),
                                    ),
                                    selected = settings.language,
                                    onSelect = { vm.setLanguage(it) },
                                )
                                Text(stringResource(R.string.settings_mode), style = MaterialTheme.typography.labelLarge)
                                Segmented(
                                    options = listOf(
                                        SegmentedOption(ThemeMode.SYSTEM, stringResource(R.string.settings_mode_system)),
                                        SegmentedOption(ThemeMode.LIGHT, stringResource(R.string.settings_mode_light)),
                                        SegmentedOption(ThemeMode.DARK, stringResource(R.string.settings_mode_dark)),
                                    ),
                                    selected = settings.themeMode,
                                    onSelect = { vm.setThemeMode(it) },
                                )
                            }
                            OnboardingStep.LISTS -> {
                                StepHeader(
                                    Icons.Outlined.Bookmarks,
                                    stringResource(R.string.settings_onboarding_lists_title),
                                    stringResource(R.string.settings_onboarding_lists_body),
                                )
                                OnboardingAccounts()
                            }
                            OnboardingStep.LINK -> StepHeader(
                                Icons.Outlined.Computer,
                                stringResource(R.string.settings_onboarding_link_title),
                                stringResource(R.string.settings_onboarding_link_body),
                            )
                            OnboardingStep.DONE -> StepHeader(
                                Icons.Outlined.Celebration,
                                stringResource(R.string.settings_onboarding_done_title),
                                stringResource(R.string.settings_onboarding_done_body),
                            )
                        }
                    }
                }
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    if (stepIndex > 0) {
                        TextButton(onClick = { stepIndex -= 1 }) { Text(stringResource(R.string.settings_onboarding_back)) }
                    }
                    Spacer(Modifier.weight(1f))
                    Button(onClick = { if (step == OnboardingStep.DONE) finish() else stepIndex += 1 }) {
                        Text(
                            stringResource(
                                when (step) {
                                    OnboardingStep.WELCOME -> R.string.settings_onboarding_start
                                    OnboardingStep.DONE -> R.string.settings_onboarding_finish
                                    else -> R.string.settings_onboarding_next
                                },
                            ),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun StepHeader(icon: ImageVector, title: String, body: String) {
    Column(Modifier.fillMaxWidth().padding(top = 24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(64.dp))
        Text(
            title,
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
            modifier = Modifier.semantics { heading() },
        )
        Text(body, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.Center)
    }
}

/** The provider cards of Settings › Listas y cuentas, without the sync / transfer rows. */
@Composable
private fun OnboardingAccounts() {
    val accountsVm: AccountsViewModel = koinViewModel()
    OnboardingProviderCards(accountsVm)
}
