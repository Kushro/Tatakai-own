// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.you

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.UserSession
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/** One line of the "Dispositivo enlazado" row. */
sealed interface LinkedSummary {
    data object None : LinkedSummary
    data class Paired(val name: String, val reachable: Boolean) : LinkedSummary
    data object Problem : LinkedSummary
}

/** Everything the "Tú" list shows (prototype android.js `S.you`). */
data class YouOverview(
    val session: UserSession?,
    val lastSyncedAt: Long?,
    val historyCount: Int,
    val extensionsCount: Int,
    val activeDownloads: Int,
    val torrentCount: Int,
    val linked: LinkedSummary,
)

data class YouUiState(
    val overview: LoadState<YouOverview> = LoadState.Loading,
    val syncing: Boolean = false,
)

sealed interface YouEvent {
    data object Synced : YouEvent
    data class SyncFailed(val cause: AppError) : YouEvent
}

/**
 * State of the You screen. [torrents] is optional so the screen works without the torrent engine bound (tests, previews).
 * History is the only flow that can fail (Room); a failure shows the error state with a retry that re-subscribes.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class YouViewModel(
    private val auth: AuthRepository,
    private val history: HistoryRepository,
    private val extensions: ExtensionRepository,
    private val link: LinkRepository,
    private val torrents: TorrentRepository? = null,
) : ViewModel() {

    private val reload = MutableStateFlow(0)
    private val syncing = MutableStateFlow(false)
    private val _events = Channel<YouEvent>(Channel.BUFFERED)
    val events: Flow<YouEvent> = _events.receiveAsFlow()

    private val historyCount: Flow<LoadState<Int>> = reload.flatMapLatest {
        history.history(HISTORY_LIMIT)
            .map { list ->
                val loaded: LoadState<Int> = LoadState.Ok(list.size)
                loaded
            }
            .catch { emit(LoadState.Err(it.toAppError())) }
    }

    private val torrentFacts: Flow<Pair<Int, Int>> = torrents?.let { repo ->
        combine(repo.status, repo.library) { status: TorrentServiceStatus, library -> status.activeDownloads to library.size }
    } ?: flowOf(0 to 0)

    private val linked: Flow<LinkedSummary> = combine(link.state, link.linkedDevice) { state, device ->
        when (state) {
            is LinkState.Paired -> LinkedSummary.Paired(device?.serverName ?: state.pc.serverName, state.reachable)
            is LinkState.Failed -> LinkedSummary.Problem
            LinkState.Pairing, LinkState.Unpaired -> LinkedSummary.None
        }
    }

    private val overview: Flow<LoadState<YouOverview>> = combine(
        combine(auth.session, history.lastSyncedAt) { session, synced -> session to synced },
        historyCount,
        extensions.installed,
        torrentFacts,
        linked,
    ) { (session, synced), count, installed, (active, total), linkedSummary ->
        when (count) {
            LoadState.Loading -> LoadState.Loading
            is LoadState.Err -> count
            is LoadState.Ok -> LoadState.Ok(
                YouOverview(
                    session = session,
                    lastSyncedAt = synced,
                    historyCount = count.value,
                    extensionsCount = installed.size,
                    activeDownloads = active,
                    torrentCount = total,
                    linked = linkedSummary,
                ),
            )
        }
    }

    val state: StateFlow<YouUiState> = combine(overview, syncing) { o, s -> YouUiState(o, s) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), YouUiState())

    fun retry() {
        reload.value++
    }

    /** "Sincronizar ahora": push pending rows and pull remote ones; the result goes to the global snackbar. */
    fun syncNow() {
        if (syncing.value) return
        syncing.value = true
        viewModelScope.launch {
            val result = runCatchingApp { history.syncNow().getOrThrow() }
            syncing.value = false
            _events.trySend(result.fold(onSuccess = { YouEvent.Synced }, onFailure = { YouEvent.SyncFailed(it.toAppError()) }))
        }
    }

    private companion object {
        const val HISTORY_LIMIT = 500
    }
}
