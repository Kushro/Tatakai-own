@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.details

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.res.Resources
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Cast
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.NoAdultContent
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.TatakaiFloatingToolbar
import app.tatakai.next.designsystem.theme.TatakaiSeedTheme
import app.tatakai.next.designsystem.theme.parseHexColor
import app.tatakai.next.feature.library.ListButton
import app.tatakai.next.feature.player.sources.SourcePickerSheet
import app.tatakai.next.feature.torrents.rememberDownloadGate
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.state.LoadStateContent
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf

@Composable
fun DetailsScreen(
    kind: MediaKind,
    anilistId: Int,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onOpenExtensions: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: DetailsViewModel = koinViewModel(
        key = "details-$kind-$anilistId",
        parameters = { parametersOf(kind, MediaRef(anilistId = anilistId)) },
    ),
    // --- android wave contract (docs/redesign/12 §3.14): new callbacks appended, wired by :app (worker b) ---
    /** After "Descargar" or from a torrent source -> Route.Torrents(OVERVIEW, torrentId). */
    onOpenTorrent: (torrentId: String) -> Unit = {},
    /** Source panel banner "Ajustes" -> Route.SettingsSection(section). */
    onOpenSettings: (SettingsSection) -> Unit = {},
    /** "Enviar a dispositivo" without a paired PC -> Route.Linked. */
    onOpenLinked: () -> Unit = {},
    /** Snackbar action after a download was queued -> Route.Torrents(DOWNLOADS). */
    onOpenDownloads: () -> Unit = {},
    /** Genre chip -> Route.SearchFor(genre = genre). */
    onOpenGenre: (String) -> Unit = {},
    // --- parity wave A (docs/redesign/15 §3.1, worker o): appended, wired by :app ---
    /** Characters tab card -> Route.Character(anilistId of the character). */
    onOpenCharacter: (characterId: Int) -> Unit = {},
    /** Studio chip of the technical sheet -> Route.SearchFor(studio = name). */
    onOpenStudio: (String) -> Unit = {},
) {
    // Related titles, seasons and recommendations open their own Details in place (the contract gives Details no "open title"
    // callback): a stack of AniList ids, popped by back. Each level is a full DetailsScreen with its own ViewModel.
    var stack by rememberSaveable(kind, anilistId) { mutableStateOf("") }
    val ids = stack.split(',').mapNotNull { it.toIntOrNull() }
    val top = ids.lastOrNull()
    if (top != null) {
        val pop = { stack = ids.dropLast(1).joinToString(",") }
        BackHandler(onBack = pop)
        DetailsScreen(
            kind = kind,
            anilistId = top,
            onBack = pop,
            onPlay = onPlay,
            onOpenExtensions = onOpenExtensions,
            modifier = modifier,
            onOpenTorrent = onOpenTorrent,
            onOpenSettings = onOpenSettings,
            onOpenLinked = onOpenLinked,
            onOpenDownloads = onOpenDownloads,
            onOpenGenre = onOpenGenre,
            onOpenCharacter = onOpenCharacter,
            onOpenStudio = onOpenStudio,
        )
        return
    }
    val openMedia: (Media) -> Unit = { media ->
        val id = media.ref.anilistId
        if (id != null && id != anilistId) stack = (ids + id).joinToString(",")
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val resources = LocalContext.current.resources
    val snackbar = LocalAppSnackbar.current
    val currentOnPlay by rememberUpdatedState(onPlay)
    val currentOnOpenDownloads by rememberUpdatedState(onOpenDownloads)
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is DetailsEvent.NavigateToPlayer -> currentOnPlay(event.args)
                is DetailsEvent.SourceChosen -> snackbar.show(
                    resources.getString(
                        R.string.details_src_using,
                        resources.getString(R.string.details_src_label, event.extName, event.label),
                    ),
                )
                is DetailsEvent.DownloadQueued -> snackbar.show(
                    message = resources.getString(R.string.details_download_queued),
                    actionLabel = resources.getString(R.string.details_download_open),
                    onAction = { currentOnOpenDownloads() },
                )
                is DetailsEvent.DownloadFailed -> snackbar.show(
                    resources.getString(R.string.details_download_failed, event.cause.describe(resources)),
                )
                DetailsEvent.NoTorrentSource -> snackbar.show(resources.getString(R.string.details_download_none))
                is DetailsEvent.SendFailed -> snackbar.show(
                    resources.getString(R.string.details_send_failed, event.cause.describe(resources)),
                )
                DetailsEvent.NoWebSource -> snackbar.show(resources.getString(R.string.details_send_no_web))
                is DetailsEvent.ListUpdateFailed -> snackbar.show(
                    resources.getString(R.string.details_mark_failed, event.cause.describe(resources)),
                )
            }
        }
    }
    // Every background download goes through the "Entiendo" gate of the torrent client (11 §4.3).
    val gate = rememberDownloadGate()
    var sendDialog by rememberSaveable { mutableStateOf(false) }
    val actions = DetailsActions(
        onBack = onBack,
        onRetry = viewModel::retry,
        onWatch = viewModel::onWatch,
        onOpenPicker = viewModel::openPicker,
        onEpisodeClick = viewModel::onEpisodeClick,
        onRetrySources = viewModel::retrySources,
        onDownloadSource = { source, episode -> gate.launch { viewModel.download(source, episode) } },
        onDownloadEpisode = { episode -> gate.launch { viewModel.downloadEpisode(episode) } },
        onDownloadSeason = { gate.launch { viewModel.downloadSeason() } },
        onSendToDevice = {
            if (state.link is LinkState.Paired) sendDialog = true else onOpenLinked()
        },
        onOpenLinked = onOpenLinked,
        onOpenExtensions = onOpenExtensions,
        onOpenSettings = onOpenSettings,
        onOpenGenre = onOpenGenre,
        onLoadRelated = viewModel::loadRelated,
        onOpenMedia = openMedia,
        onOpenCharacter = onOpenCharacter,
        onOpenStudio = onOpenStudio,
        onShare = { media ->
            shareLink(media)?.let { link ->
                context.copyToClipboard(link)
                snackbar.show(
                    message = resources.getString(R.string.details_share_copied),
                    actionLabel = resources.getString(R.string.details_share_send),
                    onAction = { context.shareText(link, resources.getString(R.string.details_share)) },
                )
            }
        },
        onOpenUrl = { url -> context.openUrl(url) },
        onLoadCharacters = viewModel::loadCharacters,
        onRetryDetails = viewModel::retryDetails,
        onEpisodeQuery = viewModel::setEpisodeQuery,
        onEpisodeView = viewModel::setEpisodeView,
        onAcceptAdult = viewModel::acceptAdult,
        onRevealCover = viewModel::revealCover,
        onToggleWatched = viewModel::toggleWatched.takeIf { viewModel.canToggleWatched },
    )
    val seed = parseHexColor((state.media as? LoadState.Ok)?.value?.color)
    TatakaiSeedTheme(seed = seed) {
        DetailsContent(state = state, actions = actions, modifier = modifier)
        state.picker?.let { request ->
            val panel = state.panel?.takeIf { it.episode == request.episode }
            SourcePickerSheet(
                sources = (panel?.sources as? LoadState.Ok)?.value.orEmpty(),
                loading = panel == null || panel.sources is LoadState.Loading,
                error = (panel?.sources as? LoadState.Err)?.cause,
                mode = state.mode,
                onPick = viewModel::onPick,
                onDismiss = viewModel::dismissPicker,
                onRetry = viewModel::retrySources,
                onOpenSettings = {
                    viewModel.dismissPicker()
                    onOpenSettings(SettingsSection.PLAYER)
                },
                onDownload = { source -> actions.onDownloadSource(source, request.episode) },
            )
        }
        if (sendDialog) {
            val media = (state.media as? LoadState.Ok)?.value
            val pc = state.pairedPc
            if (media != null && pc != null) {
                SendToDeviceDialog(
                    media = media,
                    episode = state.continueEpisode,
                    pcName = pc.serverName,
                    reachable = state.pcAvailable,
                    onConfirm = {
                        sendDialog = false
                        viewModel.sendToDevice()
                    },
                    onDismiss = { sendDialog = false },
                )
            }
        }
    }
}

/** Every callback of the Details content (the gate-wrapped download calls included). */
internal data class DetailsActions(
    val onBack: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onWatch: () -> Unit = {},
    val onOpenPicker: (playOnPick: Boolean) -> Unit = {},
    val onEpisodeClick: (Int) -> Unit = {},
    val onRetrySources: () -> Unit = {},
    val onDownloadSource: (RankedSource, Int?) -> Unit = { _, _ -> },
    val onDownloadEpisode: (Int) -> Unit = {},
    val onDownloadSeason: () -> Unit = {},
    val onSendToDevice: () -> Unit = {},
    val onOpenLinked: () -> Unit = {},
    val onOpenExtensions: () -> Unit = {},
    val onOpenSettings: (SettingsSection) -> Unit = {},
    val onOpenGenre: (String) -> Unit = {},
    val onLoadRelated: () -> Unit = {},
    // --- parity wave A (15 §5 o.2) ---
    val onOpenMedia: (Media) -> Unit = {},
    val onOpenCharacter: (Int) -> Unit = {},
    val onOpenStudio: (String) -> Unit = {},
    val onShare: (Media) -> Unit = {},
    val onOpenUrl: (String) -> Unit = {},
    val onLoadCharacters: () -> Unit = {},
    val onRetryDetails: () -> Unit = {},
    val onEpisodeQuery: (String) -> Unit = {},
    val onEpisodeView: (EpisodeView) -> Unit = {},
    val onAcceptAdult: () -> Unit = {},
    /** matureContent BLUR: tap on the blurred banner / cover reveals them. */
    val onRevealCover: () -> Unit = {},
    /** Episode row "Marcar como visto / no visto" (list progress); null = no lists layer (the button is hidden). */
    val onToggleWatched: ((Int) -> Unit)? = null,
)

@Composable
internal fun DetailsContent(
    state: DetailsUiState,
    actions: DetailsActions,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        LoadStateContent(state = state.media, onRetry = actions.onRetry) { media ->
            if (state.adultGate) AdultGate(actions) else DetailsBody(media, state, actions)
        }
        IconButton(onClick = actions.onBack, modifier = Modifier.statusBarsPadding().padding(4.dp)) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
        }
        val loaded = (state.media as? LoadState.Ok)?.value
        if (loaded != null && !state.adultGate) {
            // Floating toolbar (prototype `.ftb`): the main actions stay at hand while the episode list scrolls.
            TatakaiFloatingToolbar(
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(16.dp),
            ) {
                TextButton(onClick = actions.onWatch) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = null)
                    Text(watchLabel(state), modifier = Modifier.padding(start = 6.dp))
                }
                IconButton(onClick = actions.onSendToDevice) {
                    Icon(Icons.Outlined.Cast, contentDescription = stringResource(R.string.details_send_device))
                }
                DownloadMenuButton(state, actions, icon = Icons.Outlined.Download)
                ListButton(media = loaded, compact = true)
            }
        }
    }
}

@Composable
internal fun watchLabel(state: DetailsUiState): String = when {
    !state.hasProgress -> stringResource(R.string.details_watch_ep, state.continueEpisode)
    else -> stringResource(R.string.details_continue, state.continueEpisode)
}

/** Phone: one column (header, source panel, tabs). Wide (≥ 840 dp): tabs on the left, source + send panels on the right. */
@Composable
private fun DetailsBody(media: Media, state: DetailsUiState, actions: DetailsActions) {
    var tab by rememberSaveable { mutableIntStateOf(DetailsTab.EPISODES.ordinal) }
    var range by rememberSaveable(media.ref) { mutableIntStateOf((state.continueEpisode - 1).coerceAtLeast(0) / RANGE_SIZE) }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val wide = maxWidth >= WIDE_BREAKPOINT
        val selectedTab = DetailsTab.entries[tab]
        if (wide) {
            Row(Modifier.fillMaxSize()) {
                LazyColumn(Modifier.weight(1f).fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
                    detailsItems(media, state, actions, selectedTab, { tab = it.ordinal }, range, { range = it }, includePanel = false)
                }
                Column(
                    Modifier
                        .width(SIDE_WIDTH)
                        .fillMaxSize()
                        .verticalScroll(rememberScrollState())
                        .statusBarsPadding()
                        .padding(16.dp)
                        .padding(bottom = 96.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    SourcePanelCard(state, actions)
                    SendToDevicePanel(state, actions)
                }
            }
        } else {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 96.dp)) {
                detailsItems(media, state, actions, selectedTab, { tab = it.ordinal }, range, { range = it }, includePanel = true)
            }
        }
    }
}

@Composable
private fun SendToDeviceDialog(
    media: Media,
    episode: Int,
    pcName: String,
    reachable: Boolean,
    onConfirm: () -> Unit,
    onDismiss: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Cast, contentDescription = null) },
        title = { Text(stringResource(R.string.details_send_device)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.details_send_body, media.title, episode, pcName))
                if (!reachable) {
                    Text(
                        stringResource(R.string.details_send_unreachable),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.error,
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.details_send_action)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.details_cancel)) } },
    )
}

/** Non-composable twin of `AppError.userMessage()` for snackbars raised from events. */
internal fun AppError.describe(resources: Resources): String = when (this) {
    AppError.Offline -> resources.getString(UiR.string.ui_error_offline)
    is AppError.RateLimited -> resources.getString(UiR.string.ui_error_rate_limited, provider)
    is AppError.Extension -> resources.getString(UiR.string.ui_error_extension, id, msg)
    is AppError.Http -> resources.getString(UiR.string.ui_error_http, code)
    is AppError.Unknown -> resources.getString(UiR.string.ui_error_unknown, msg)
}

/** Classic AdultWarningGate: an 18+ title asks once; "Mostrar" is remembered per title (DetailsPrefs). */
@Composable
private fun AdultGate(actions: DetailsActions) {
    Column(
        Modifier.fillMaxSize().statusBarsPadding().padding(24.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        EmptyState(
            icon = Icons.Outlined.NoAdultContent,
            title = stringResource(R.string.details_adult_title),
            message = stringResource(R.string.details_adult_body),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            OutlinedButton(onClick = actions.onBack) { Text(stringResource(R.string.details_adult_back)) }
            Button(onClick = actions.onAcceptAdult) { Text(stringResource(R.string.details_adult_show)) }
        }
    }
}

private fun Context.copyToClipboard(text: String) {
    getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText("Tatakai", text))
}

private fun Context.shareText(text: String, title: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
    runCatching { startActivity(Intent.createChooser(send, title).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
}

/** Trailer (YouTube / Dailymotion) and external links: the system picks the app (`ACTION_VIEW`). */
private fun Context.openUrl(url: String) {
    runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
}

internal enum class DetailsTab { EPISODES, INFO, CHARACTERS, RELATED }

internal const val RANGE_SIZE = 100
private val WIDE_BREAKPOINT = 840.dp
private val SIDE_WIDTH = 380.dp
