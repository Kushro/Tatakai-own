package app.tatakai.next.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onStart
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/**
 * Library (E7, prototype S.library): local only. Continuar = one row per series (continueWatching), Historial = one row per watched
 * episode (HistoryRepository.history), Torrents = the torrent client tab (badge = active downloads). AniList/MAL lists arrive in M1.
 */
data class LibraryUiState(
    val continueWatching: LoadState<List<HistoryEntry>> = LoadState.Loading,
    val history: LoadState<List<HistoryEntry>> = LoadState.Loading,
    /** TorrentServiceStatus.activeDownloads: the Torrents tab badge. */
    val activeDownloads: Int = 0,
    val syncing: Boolean = false,
    /** Epoch ms of the last successful sync; null = never / no account. */
    val lastSyncedAt: Long? = null,
)

sealed interface LibraryEvent {
    data object Synced : LibraryEvent

    data class SyncFailed(val cause: AppError) : LibraryEvent
}

/** Newest first. */
internal fun List<HistoryEntry>.newestFirst(): List<HistoryEntry> = sortedByDescending { it.watchedAt }

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryViewModel(
    private val history: HistoryRepository,
    torrents: TorrentRepository? = null,
) : ViewModel() {

    private val reload = MutableStateFlow(0)
    private val syncing = MutableStateFlow(false)

    private val _events = Channel<LibraryEvent>(Channel.BUFFERED)
    val events: Flow<LibraryEvent> = _events.receiveAsFlow()

    private val continueFlow: Flow<LoadState<List<HistoryEntry>>> = reload.flatMapLatest {
        history.continueWatching(CONTINUE_LIMIT).asLoadState()
    }

    private val historyFlow: Flow<LoadState<List<HistoryEntry>>> = reload.flatMapLatest {
        history.history(HISTORY_LIMIT).asLoadState()
    }

    private val downloads: Flow<Int> = torrents?.status
        ?.map { it.activeDownloads }
        ?.distinctUntilChanged()
        ?.catch { emit(0) }
        ?: flowOf(0)

    private val lastSynced: Flow<Long?> = history.lastSyncedAt.catch { emit(null) }

    val state: StateFlow<LibraryUiState> = combine(
        continueFlow,
        historyFlow,
        downloads,
        syncing,
        lastSynced,
    ) { cont, hist, active, sync, last ->
        LibraryUiState(continueWatching = cont, history = hist, activeDownloads = active, syncing = sync, lastSyncedAt = last)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LibraryUiState())

    fun retry() {
        reload.value += 1
    }

    /** "Sincronizar ahora": push pending rows and pull remote ones (no-op success without an account). */
    fun syncNow() {
        if (syncing.value) return
        syncing.value = true
        viewModelScope.launch {
            val result = runCatchingApp { history.syncNow().getOrThrow() }
            syncing.value = false
            _events.trySend(
                result.fold(
                    onSuccess = { LibraryEvent.Synced },
                    onFailure = { LibraryEvent.SyncFailed(it.toAppError()) },
                ),
            )
        }
    }

    private fun Flow<List<HistoryEntry>>.asLoadState(): Flow<LoadState<List<HistoryEntry>>> =
        map<List<HistoryEntry>, LoadState<List<HistoryEntry>>> { LoadState.Ok(it.newestFirst()) }
            .onStart { emit(LoadState.Loading) }
            .catch { e ->
                if (e is CancellationException) throw e
                emit(LoadState.Err(e.toAppError()))
            }

    private companion object {
        const val CONTINUE_LIMIT = 50
        const val HISTORY_LIMIT = 200
    }
}
