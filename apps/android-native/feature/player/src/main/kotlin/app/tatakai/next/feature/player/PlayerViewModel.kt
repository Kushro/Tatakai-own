// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.PlaybackController
import app.tatakai.next.player.PlaybackControllerFactory
import app.tatakai.next.player.PlaybackListener
import app.tatakai.next.player.ProgressTracker
import app.tatakai.next.player.RemotePlayback
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.time.Instant

sealed interface PlayerUiState {
    data object ResolvingSources : PlayerUiState
    data object NoSources : PlayerUiState
    data object RemoteUnavailable : PlayerUiState
    data class Failed(val retryable: Boolean) : PlayerUiState
    data class Ready(val controller: PlaybackController, val sourceLabels: List<String>) : PlayerUiState
}

/**
 * Resolves what to play (extension streams for the episode, or the Link HLS of `args.remoteSessionId`), owns the
 * [PlaybackController] and reports progress to [HistoryRepository] (E9 keys). Survives rotation.
 */
@UnstableApi
class PlayerViewModel(
    private val args: PlayerArgs,
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val history: HistoryRepository,
    private val link: LinkRepository,
    private val remote: RemotePlaybackAccess,
    private val factory: PlaybackControllerFactory,
    private val flush: HistoryFlush?,
) : ViewModel() {
    private val _state = MutableStateFlow<PlayerUiState>(PlayerUiState.ResolvingSources)
    val state: StateFlow<PlayerUiState> = _state.asStateFlow()

    /** Survives `viewModelScope` cancellation: last progress row and the remote `DELETE` must still go out on exit. */
    private val detached = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var poster: String? = null
    private var controller: PlaybackController? = null

    init {
        resolve()
    }

    fun retry() {
        controller?.release()
        controller = null
        _state.value = PlayerUiState.ResolvingSources
        resolve()
    }

    private fun resolve() {
        viewModelScope.launch {
            val remoteId = args.remoteSessionId
            val sources: List<PlayableSource> = try {
                if (remoteId != null) {
                    listOfNotNull(RemotePlayback.source(remote, remoteId, args.title)).also {
                        if (it.isEmpty()) {
                            _state.value = PlayerUiState.RemoteUnavailable
                            return@launch
                        }
                    }
                } else {
                    val media = metadata.media(MediaKind.ANIME, args.media)
                    if (media == null) {
                        _state.value = PlayerUiState.Failed(retryable = true)
                        return@launch
                    }
                    poster = media.cover
                    extensions.resolveStreams(media, args.episode).map(PlayableSource::from)
                }
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                _state.value = PlayerUiState.Failed(retryable = true)
                return@launch
            }
            if (sources.isEmpty()) {
                _state.value = PlayerUiState.NoSources
                return@launch
            }
            val c = factory.create(
                sources = sources,
                scope = viewModelScope,
                listener = listener(remoteId),
                remote = remoteId != null,
                startMs = args.startMs,
            )
            controller = c
            _state.value = PlayerUiState.Ready(c, sources.map { it.label })
        }
    }

    private fun listener(remoteId: String?) = object : PlaybackListener {
        override fun onProgress(positionMs: Long, durationMs: Long, force: Boolean) {
            val entry = ProgressTracker.entry(
                anilistId = args.media.anilistId,
                malId = args.media.malId,
                title = args.title,
                poster = poster,
                episode = args.episode,
                positionMs = positionMs,
                durationMs = durationMs,
                now = Instant.now(),
            ) ?: return
            if (positionMs <= 0) return
            detached.launch {
                runCatching { history.record(entry) }
                if (force) flush?.requestImmediate()
            }
        }

        override fun onHeartbeat(positionMs: Long, paused: Boolean) {
            val id = remoteId ?: return
            detached.launch { remote.heartbeat(id, positionMs / 1000.0, paused) }
        }
    }

    override fun onCleared() {
        controller?.release() // emits the final forced progress tick
        controller = null
        args.remoteSessionId?.let { id -> detached.launch { link.stop(id) } }
    }
}
