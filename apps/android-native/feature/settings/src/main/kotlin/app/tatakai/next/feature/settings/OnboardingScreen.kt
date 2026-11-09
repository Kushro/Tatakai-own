// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.settings

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.Palette
import androidx.compose.material.icons.outlined.PlayCircle
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.ThemeMode
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

/** Steps of the first-run wizard, in order ([onboardingSteps] drops the ones a device does not need). */
internal enum class OnboardingStep { WELCOME, APPEARANCE, PLAYBACK, LISTS, LINK, PERMISSIONS, DONE }

/**
 * The wizard's steps on this device (checklist account.setup-wizard: language, engine and source preferences, AniList / MAL, permissions).
 * PERMISSIONS only when a runtime permission is still worth asking: notifications need one from Android 13 (API 33). Pure, unit tested.
 */
internal fun onboardingSteps(notificationsNeedPermission: Boolean): List<OnboardingStep> =
    OnboardingStep.entries.filter { it != OnboardingStep.PERMISSIONS || notificationsNeedPermission }

/** First-run wizard state: writes language / theme at once and `onboardingDone = true` at the end (or on Skip). */
class OnboardingViewModel(private val settingsRepo: SettingsRepository) : ViewModel() {
    val settings: StateFlow<AppSettings> = settingsRepo.settings

    fun setLanguage(value: AppLanguage) = viewModelScope.launch { settingsRepo.update { it.copy(language = value) } }

    fun setThemeMode(value: ThemeMode) = viewModelScope.launch { settingsRepo.update { it.copy(themeMode = value) } }

    // Playback step: the same fields as Settings › Reproductor (engine and source preferences).
    fun setPlayerEngine(value: PlayerEngine) = viewModelScope.launch { settingsRepo.update { it.copy(playerEngine = value) } }

    fun setSourceMode(value: SourceMode) = updatePrefs { it.copy(mode = value) }

    /** null = any kind. */
    fun setPreferredKind(value: SourceKind?) = updatePrefs { it.copy(prefer = value) }

    fun setMinQuality(value: Quality) = updatePrefs { it.copy(minQuality = value) }

    fun setSubLang(value: String) = updatePrefs { it.copy(subLang = value) }

    fun setDubLang(value: String) = updatePrefs { it.copy(dubLang = value) }

    private fun updatePrefs(transform: (SourcePrefs) -> SourcePrefs) =
        viewModelScope.launch { settingsRepo.update { it.copy(sourcePrefs = transform(it.sourcePrefs)) } }

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
 * First-run wizard (classic onboarding + setup wizard, trimmed to what Next has): welcome → language and theme → player engine and
 * source preferences → connect AniList / MAL (optional, Custom Tab through the same [AccountsViewModel] as Settings) → link a PC
 * (optional; explains where pairing lives) → permissions (notifications, Android 13+) → done.
 * Every step can be skipped. The last step (or Skip) writes `AppSettings.onboardingDone = true`; the shell reacts to the flag and
 * [onDone] is an extra hook. Shown by `TatakaiApp` while `onboardingDone` is false, so it runs again after "Restablecer la app".
 */
@Composable
fun OnboardingScreen(onDone: () -> Unit, modifier: Modifier = Modifier) {
    val vm: OnboardingViewModel = koinViewModel()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val steps = remember { onboardingSteps(notificationsNeedPermission = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) }
    var stepIndex by rememberSaveable { mutableIntStateOf(0) }
    val step = steps[stepIndex.coerceIn(0, steps.lastIndex)]
    val finish = { vm.finish(onDone) }

    Surface(modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
        Box(Modifier.fillMaxSize().safeDrawingPadding(), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier.fillMaxSize().widthIn(max = 560.dp).padding(horizontal = 24.dp, vertical = 16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    LinearProgressIndicator(
                        progress = { (stepIndex + 1f) / steps.size },
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
                            OnboardingStep.PLAYBACK -> {
                                StepHeader(
                                    Icons.Outlined.PlayCircle,
                                    stringResource(R.string.settings_onboarding_playback_title),
                                    stringResource(R.string.settings_onboarding_playback_body),
                                )
                                OnboardingPlayback(settings, vm)
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
                            OnboardingStep.PERMISSIONS -> {
                                StepHeader(
                                    Icons.Outlined.NotificationsActive,
                                    stringResource(R.string.settings_onboarding_permissions_title),
                                    stringResource(R.string.settings_onboarding_permissions_body),
                                )
                                OnboardingNotifications()
                            }
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

/** Engine and source preferences, the same options as Settings › Reproductor (the rest of that section stays there). */
@Composable
private fun OnboardingPlayback(settings: AppSettings, vm: OnboardingViewModel) {
    val prefs = settings.sourcePrefs
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.settings_engine), style = MaterialTheme.typography.labelLarge)
        Segmented(
            options = listOf(
                SegmentedOption(PlayerEngine.MEDIA3, stringResource(R.string.settings_engine_media3)),
                SegmentedOption(PlayerEngine.MPV, stringResource(R.string.settings_engine_mpv)),
                SegmentedOption(PlayerEngine.EXTERNAL, stringResource(R.string.settings_engine_external)),
            ),
            selected = settings.playerEngine,
            onSelect = { vm.setPlayerEngine(it) },
        )
        Text(
            stringResource(settings.playerEngine.hintRes()),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(stringResource(R.string.settings_source_mode), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
        Segmented(
            options = listOf(
                SegmentedOption(SourceMode.AUTO, stringResource(UiR.string.src_mode_auto)),
                SegmentedOption(SourceMode.MANUAL, stringResource(UiR.string.src_mode_manual)),
            ),
            selected = prefs.mode,
            onSelect = { vm.setSourceMode(it) },
        )
        Text(
            stringResource(R.string.settings_source_mode_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Text(stringResource(R.string.settings_prefer), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
        Segmented(
            options = listOf<SegmentedOption<SourceKind?>>(
                SegmentedOption(null, stringResource(R.string.settings_prefer_any)),
                SegmentedOption(SourceKind.WEB, stringResource(UiR.string.src_web)),
                SegmentedOption(SourceKind.TORRENT, stringResource(UiR.string.src_torrent)),
            ),
            selected = prefs.prefer,
            onSelect = { vm.setPreferredKind(it) },
            small = true,
        )
        Text(stringResource(R.string.settings_min_quality), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
        Segmented(
            options = listOf(Quality.P480, Quality.P720, Quality.P1080).map { SegmentedOption(it, it.label()) },
            selected = prefs.minQuality,
            onSelect = { vm.setMinQuality(it) },
            small = true,
        )
        Text(stringResource(R.string.settings_sub_lang), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
        Segmented(options = langOptions(listOf("es", "en", "pt")), selected = prefs.subLang, onSelect = { vm.setSubLang(it) }, small = true)
        Text(stringResource(R.string.settings_dub_lang), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
        Segmented(options = langOptions(listOf("ja", "es", "en")), selected = prefs.dubLang, onSelect = { vm.setDubLang(it) }, small = true)
    }
}

/**
 * Notifications (Android 13+): torrent downloads in the background and the playback controls. Asking is optional; a denial only says
 * where to turn them on later (the torrent client asks again when background downloads are enabled).
 */
@Composable
private fun OnboardingNotifications() {
    val context = LocalContext.current
    fun granted() = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
        ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
    var allowed by remember { mutableStateOf(granted()) }
    var asked by rememberSaveable { mutableStateOf(false) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok ->
        allowed = ok || granted()
        asked = true
    }
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(Icons.Outlined.Notifications, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.settings_onboarding_notifications), style = MaterialTheme.typography.titleSmall)
                Text(
                    stringResource(
                        when {
                            allowed -> R.string.settings_onboarding_notifications_granted
                            asked -> R.string.settings_onboarding_notifications_denied
                            else -> R.string.settings_onboarding_notifications_hint
                        },
                    ),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (allowed) {
                Icon(Icons.Outlined.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                FilledTonalButton(onClick = { launcher.launch(Manifest.permission.POST_NOTIFICATIONS) }) {
                    Text(stringResource(R.string.settings_onboarding_notifications_allow))
                }
            }
        }
    }
}

/** The provider cards of Settings › Listas y cuentas, without the sync / transfer rows. */
@Composable
private fun OnboardingAccounts() {
    val accountsVm: AccountsViewModel = koinViewModel()
    OnboardingProviderCards(accountsVm)
}
