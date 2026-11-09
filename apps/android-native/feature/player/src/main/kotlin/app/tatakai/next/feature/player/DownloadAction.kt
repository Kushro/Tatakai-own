// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.DownloadDone
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.res.stringResource
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadRequest
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.StreamSource
import app.tatakai.next.ui.R as UiR
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import org.koin.compose.getKoin
import androidx.lifecycle.compose.collectAsStateWithLifecycle

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r, checklist downloads.player-button): "Descargar" in the
 * player bar. Queues the episode that is playing with the stream that is playing ([current], `SourceCandidate.toStream()`; embed sources
 * arrive here already sniffed) through `DownloadRepository.enqueue`. Hidden for local files, torrents (they have their own client),
 * Link sessions, DASH and when no download module is bound. Shows "downloaded" once the episode is in Downloads.
 */
@Composable
fun DownloadAction(args: PlayerArgs, current: StreamSource?, label: String) {
    if (args.localUri != null || args.torrentId != null || args.remoteSessionId != null) return
    if (current == null || current.container == Container.DASH) return
    val koin = getKoin()
    val downloads = remember { koin.getOrNull<DownloadRepository>() } ?: return
    val metadata = remember { koin.getOrNull<MetadataRepository>() }
    val id = remember(args.media, args.episode) { args.media to args.episode }
    val stateFlow = remember(id) {
        downloads.items.map { list ->
            list.firstOrNull { it.episode == args.episode && sameMedia(it.ref, args.media) }?.state
        }
    }
    val state by stateFlow.collectAsStateWithLifecycle(initialValue = null)
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val snackbar = LocalAppSnackbar.current
    val queued = stringResource(UiR.string.download_queued)
    val failed = stringResource(UiR.string.download_state_failed)
    val on = MaterialTheme.colorScheme.onBackground
    val done = state == DownloadState.COMPLETED
    val inQueue = state == DownloadState.QUEUED || state == DownloadState.DOWNLOADING || state == DownloadState.PAUSED
    IconButton(
        enabled = !busy && !done && !inQueue,
        onClick = {
            busy = true
            scope.launch {
                val media = runCatching { metadata?.media(MediaKind.ANIME, args.media) }.getOrNull() ?: fallbackMedia(args)
                downloads.enqueue(DownloadRequest(media = media, episode = args.episode, stream = current, sourceLabel = label)).fold(
                    onSuccess = { snackbar.show(queued) },
                    onFailure = { e -> snackbar.show("$failed: ${e.message ?: e::class.java.simpleName}") },
                )
                busy = false
            }
        },
    ) {
        Icon(
            if (done) Icons.Rounded.DownloadDone else Icons.Rounded.Download,
            contentDescription = stringResource(if (done) UiR.string.download_state_completed else UiR.string.download_episode),
            tint = on,
        )
    }
}

/** The [PlayerArgs] of the screen for [DownloadAction] in the control bar (provided by `PlayerBody`). */
val LocalDownloadArgs: ProvidableCompositionLocal<PlayerArgs?> = staticCompositionLocalOf { null }

private fun sameMedia(a: app.tatakai.next.model.MediaRef, b: app.tatakai.next.model.MediaRef): Boolean =
    (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId)

private fun fallbackMedia(args: PlayerArgs) = Media(
    ref = args.media, kind = MediaKind.ANIME, title = args.title, titles = emptyMap(), cover = null, banner = null, color = null,
    synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null, sourceProvider = "player",
)
