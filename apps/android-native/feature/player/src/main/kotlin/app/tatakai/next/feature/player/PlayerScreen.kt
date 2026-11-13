// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(UnstableApi::class)

package app.tatakai.next.feature.player

import android.content.ActivityNotFoundException
import androidx.activity.compose.BackHandler
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Devices
import androidx.compose.material.icons.rounded.Extension
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiTheme
import app.tatakai.next.feature.player.sources.SourcePickerSheet
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.torrent.DebridRepository
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.player.ExternalPlayer
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.MPV_SESSION_FACTORY
import app.tatakai.next.player.PlayerPrefs
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.SubtitleTools
import app.tatakai.next.ui.state.contrastLevel
import app.tatakai.next.ui.state.rememberIsOnline
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.getKoin
import org.koin.core.qualifier.named
import app.tatakai.next.ui.R as UiR

/**
 * Full-screen player. [args] with `remoteSessionId` plays the HLS of that Link session through the mTLS client of the pairing; `localUri`
 * plays a completed download offline; `torrentId` plays a file of a torrent already in the session; otherwise sources come from the
 * installed extensions (Auto / Manual, 12 §4 d). The engine is `AppSettings.playerEngine` (Media3, libmpv or an external app). Progress
 * goes to the history (E9 keys) and the screen survives rotation. The host Activity needs `android:supportsPictureInPicture="true"` and
 * `configChanges` for orientation|screenSize for PiP to work. Drawn in a dark AMOLED variant of the app palette (video chrome).
 */
@OptIn(UnstableApi::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PlayerScreen(
    args: PlayerArgs,
    onBack: () -> Unit,
    // --- android wave contract (docs/redesign/12 §3.11): new callbacks, wired by :app (worker b), used by worker d ---
    /** Replace this player entry with another episode / source (next episode, batch next, source change that needs a new route). */
    onReplace: (PlayerArgs) -> Unit = {},
    /** Chip "Abrir en el cliente" -> Route.Torrents(OVERVIEW, torrentId). */
    onOpenTorrent: (torrentId: String) -> Unit = {},
    /** Source picker banner "Ajustes" / metered dialog -> Route.SettingsSection(section). */
    onOpenSettings: (SettingsSection) -> Unit = {},
    onOpenExtensions: () -> Unit = {},
    onOpenLinked: () -> Unit = {},
) {
    val koin = getKoin()
    val vm: PlayerViewModel = viewModel(
        key = "player-${args.media.anilistId}-${args.media.malId}-${args.episode}-${args.remoteSessionId}-${args.sourceKey}-" +
            "${args.torrentId}-${args.fileIndex}-${args.localUri}-${args.provider?.historyId}-${args.provider?.entryId}",
        factory = viewModelFactory {
            initializer {
                PlayerViewModel(
                    args = args,
                    metadata = koin.get(),
                    extensions = koin.get(),
                    history = koin.get(),
                    link = koin.get(),
                    remote = koin.get(),
                    media3 = koin.get(),
                    flush = koin.getOrNull<HistoryFlush>(),
                    settingsRepository = koin.getOrNull(),
                    torrents = koin.getOrNull(),
                    stats = koin.getOrNull(),
                    skipMarks = koin.getOrNull(),
                    mpvFactory = koin.getOrNull<PlayerSessionFactory>(named(MPV_SESSION_FACTORY)),
                    prefs = koin.getOrNull<PlayerPrefs>(),
                    subtitleTools = koin.getOrNull<SubtitleTools>(),
                    secrets = koin.getOrNull<SecretStore>(),
                    debrid = koin.getOrNull<DebridRepository>(),
                    embedSniffer = koin.getOrNull<EmbedSniffer>(),
                )
            }
        },
    )
    val settingsRepo = remember { koin.getOrNull<SettingsRepository>() }
    val appSettings = settingsRepo?.settings?.collectAsStateWithLifecycle()?.value
    val state by vm.state.collectAsStateWithLifecycle()
    val settings = appSettings ?: state.settings

    LaunchedEffect(vm) { vm.replace.collect { onReplace(it) } }
    // Media notification / lock screen (PlaybackService): title + "Episode N".
    val notificationLine = if (args.remoteSessionId == null) stringResource(R.string.player_next_episode_n, args.episode) else null
    LaunchedEffect(vm, notificationLine) { vm.setNotificationText(args.title, notificationLine) }

    TatakaiTheme(
        paletteId = settings.paletteId,
        amoled = true,
        contrast = settings.contrastLevel(),
        dynamicColor = settings.dynamicColor,
        darkTheme = true,
    ) {
        PlayerBody(
            vm = vm,
            state = state,
            args = args,
            onBack = onBack,
            onOpenTorrent = { id -> vm.pauseForClient(); onOpenTorrent(id) },
            onOpenSettings = onOpenSettings,
            onOpenExtensions = onOpenExtensions,
            onOpenLinked = onOpenLinked,
        )
    }
    BackHandler(enabled = true, onBack = onBack)
}

@OptIn(UnstableApi::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun PlayerBody(
    vm: PlayerViewModel,
    state: PlayerUiState,
    args: PlayerArgs,
    onBack: () -> Unit,
    onOpenTorrent: (String) -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
) {
    val title = if (args.remoteSessionId != null) args.title else stringResource(R.string.player_episode_title, args.title, args.episode)
    var pickerOpen by rememberSaveable { mutableStateOf(false) }
    var manualPrompted by rememberSaveable { mutableStateOf(false) }
    var flash by remember { mutableStateOf<PlayerNotice?>(null) }

    LaunchedEffect(vm) { vm.notices.collect { flash = it } }
    // Link session: when the network drops for longer than the player's retries, the session comes back by itself once the
    // network returns (same as tapping Retry; the PC keeps the session 120 s). Only an offline -> online change retries, so an
    // error the PC itself returns never loops.
    if (args.remoteSessionId != null) {
        val online = rememberIsOnline()
        var wentOffline by remember { mutableStateOf(false) }
        LaunchedEffect(online) {
            if (!online) {
                wentOffline = true
            } else if (wentOffline) {
                wentOffline = false
                if (vm.state.value.phase == PlayerPhase.ERROR) vm.retry()
            }
        }
    }
    // Manual mode: open the picker once by itself (prototype: 300 ms after entering).
    LaunchedEffect(state.phase) {
        if (state.phase == PlayerPhase.MANUAL && !manualPrompted) {
            manualPrompted = true
            kotlinx.coroutines.delay(300)
            pickerOpen = true
        }
    }

    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        val session = state.session
        when {
            state.phase == PlayerPhase.EXTERNAL && state.external != null -> ExternalBranch(vm, state, title, onBack, onOpenSettings)
            session != null && state.phase != PlayerPhase.ERROR -> androidx.compose.runtime.CompositionLocalProvider(LocalDownloadArgs provides args) { PlayerContent(
                vm = vm,
                session = session,
                state = state,
                title = title,
                flash = flash,
                onFlashDone = { flash = null },
                onBack = onBack,
                onOpenPicker = { pickerOpen = true },
                onOpenTorrent = onOpenTorrent,
            ) }
            else -> StatusBody(vm, state, onOpenTorrent, onOpenExtensions, onOpenLinked, onPick = { pickerOpen = true })
        }
        // Back is always reachable, also while sources load or fail.
        if (session == null || state.phase == PlayerPhase.ERROR) {
            if (state.phase != PlayerPhase.EXTERNAL) {
                IconButton(onClick = onBack, Modifier.statusBarsPadding().padding(8.dp).align(Alignment.TopStart)) {
                    Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), tint = MaterialTheme.colorScheme.onBackground)
                }
            }
            if (session == null) {
                flash?.let { FlashToast(it, onDone = { flash = null }, modifier = Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 16.dp)) }
            }
        }
    }

    state.metered?.let { prompt ->
        MeteredDialog(
            prompt = prompt,
            onWatch = vm::acceptMetered,
            onWeb = vm::declineMetered,
            onSettings = { vm.declineMetered(); onOpenSettings(SettingsSection.TORRENTS) },
        )
    }

    if (pickerOpen) {
        SourcePickerSheet(
            sources = state.sources,
            loading = state.candidatesLoading,
            error = state.candidatesError,
            mode = state.mode,
            onPick = { s -> pickerOpen = false; vm.pickSource(s.key) },
            onDismiss = { pickerOpen = false },
            onRetry = vm::reloadSources,
            onOpenSettings = { pickerOpen = false; onOpenSettings(SettingsSection.PLAYER) },
        )
    }
}

/** Everything that is not a running engine: loading, Manual, no sources, errors. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun StatusBody(
    vm: PlayerViewModel,
    state: PlayerUiState,
    onOpenTorrent: (String) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
    onPick: () -> Unit,
) {
    val onColor = MaterialTheme.colorScheme.onBackground
    when (state.phase) {
        PlayerPhase.LOADING, PlayerPhase.RESOLVING, PlayerPhase.BUFFERING, PlayerPhase.PLAYING -> {
            val show = rememberAfterDelay(150)
            if (show) {
                Centered {
                    LoadingIndicator(Modifier.size(56.dp))
                    val torrent = state.torrent
                    if (torrent != null) {
                        TorrentStatusText(torrent)
                    } else {
                        val name = state.current?.candidate?.let { "${it.extName} · ${it.label}" }
                        Text(
                            if (name != null) stringResource(R.string.player_resolving_from, name) else stringResource(R.string.player_loading_sources),
                            color = onColor,
                            textAlign = TextAlign.Center,
                        )
                    }
                    state.position?.let { (n, total) ->
                        Text(stringResource(R.string.player_source_n, n, total), color = onColor.copy(alpha = 0.7f), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        }
        PlayerPhase.MANUAL -> Centered {
            Text(stringResource(R.string.src_picker_choose_first), color = onColor, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
            FilledTonalButton(onClick = onPick) { Text(stringResource(R.string.player_pick_source)) }
        }
        PlayerPhase.NO_SOURCES -> Centered {
            EmptyState(
                icon = Icons.Rounded.Extension,
                title = stringResource(R.string.player_no_sources_title),
                message = stringResource(R.string.player_no_sources),
                actionLabel = stringResource(R.string.player_go_extensions),
                onAction = onOpenExtensions,
            )
        }
        PlayerPhase.REMOTE_UNAVAILABLE -> Centered {
            EmptyState(
                icon = Icons.Rounded.Devices,
                title = stringResource(R.string.player_remote_unavailable),
                actionLabel = stringResource(R.string.player_open_linked),
                onAction = onOpenLinked,
            )
        }
        PlayerPhase.FAILED -> Centered {
            ErrorState(
                message = state.error?.userMessage() ?: stringResource(R.string.player_error),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = vm::retry,
                title = stringResource(R.string.player_error),
            )
        }
        PlayerPhase.ERROR -> PlaybackErrorView(
            state = state,
            onRetry = vm::retry,
            onUseWeb = vm::useBestWeb,
            onPick = onPick,
            onOpenTorrent = onOpenTorrent,
            onOpenExtensions = onOpenExtensions,
        )
        PlayerPhase.EXTERNAL -> Unit
    }
}

/** The EXTERNAL engine: launches the other app once, then shows the informative screen until Stop / Back. */
@Composable
private fun ExternalBranch(
    vm: PlayerViewModel,
    state: PlayerUiState,
    title: String,
    onBack: () -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
) {
    val external = state.external ?: return
    val context = LocalContext.current
    // Like desktop external mpv: when the other app closes, record the progress it reports and go back (09 §4.4).
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        vm.onExternalResult(ExternalPlayer.resultPositionMs(result.data), ExternalPlayer.resultDurationMs(result.data))
        onBack()
    }
    val chooserTitle = stringResource(R.string.player_external_chooser)
    fun open() {
        val ok = try {
            launcher.launch(ExternalPlayer.intent(external.launch, chooserTitle))
            true
        } catch (e: ActivityNotFoundException) {
            false
        }
        vm.onExternalLaunched(ok)
    }
    LaunchedEffect(external.launch) { if (!external.launched) open() }
    val appLabel = remember(external.launch.packageName) { ExternalPlayer.appLabel(context, external.launch.packageName) }
    if (external.failed) {
        Centered {
            ErrorState(
                message = stringResource(R.string.player_external_no_app),
                retryLabel = stringResource(R.string.player_external_open_settings),
                onRetry = { onOpenSettings(SettingsSection.PLAYER) },
                title = stringResource(R.string.player_error),
            )
        }
        return
    }
    ExternalPlayerContent(
        title = title,
        appLabel = appLabel,
        onStop = onBack,
        onBack = onBack,
        torrentLine = state.torrent?.let { torrentLine(it) },
        onReopen = { open() },
    )
}

@Composable
internal fun Centered(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().padding(24.dp), Arrangement.spacedBy(16.dp, Alignment.CenterVertically), Alignment.CenterHorizontally) { content() }
}
