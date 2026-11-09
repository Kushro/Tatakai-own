// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.you

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
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
import org.koin.core.context.GlobalContext

/** One line of the "Dispositivo enlazado" row. */
sealed interface LinkedSummary {
    data object None : LinkedSummary
    data class Paired(val name: String, val reachable: Boolean) : LinkedSummary
    data object Problem : LinkedSummary
}

/**
 * Everything the "Tú" list shows (prototype android.js `S.you`). 2026-11-09 amendment (plan 14 §0.1 #9): no own login; the identity is
 * [accounts] (AniList / MAL, `ListsRepository`) and [lastSyncedAt] is the history sync with the paired PC over Tatakai Link.
 */
data class YouOverview(
    val accounts: List<LinkedAccount>,
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
    private val lists: ListsRepository,
    private val history: HistoryRepository,
    private val extensions: ExtensionRepository,
    private val link: LinkRepository,
    private val torrents: TorrentRepository? = null,
    /**
     * parity wave B (16 §2, worker r): web downloads of :core:download, added to [YouOverview.activeDownloads]. Read from Koin when the
     * module is loaded (youFeatureModule is closed and does not pass it); null in tests / previews.
     */
    private val downloads: DownloadRepository? = runCatching { GlobalContext.getOrNull()?.getOrNull<DownloadRepository>() }.getOrNull(),
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

    private val torrentOnly: Flow<Pair<Int, Int>> = torrents?.let { repo ->
        combine(repo.status, repo.library) { status: TorrentServiceStatus, library -> status.activeDownloads to library.size }
    } ?: flowOf(0 to 0)

    /** Active torrents + active web downloads (queued / downloading), and the torrent library size. */
    private val torrentFacts: Flow<Pair<Int, Int>> = downloads?.let { repo ->
        combine(torrentOnly, repo.summary) { (active, total), web -> (active + web.active) to total }
    } ?: torrentOnly

    private val linked: Flow<LinkedSummary> = combine(link.state, link.linkedDevice) { state, device ->
        when (state) {
            is LinkState.Paired -> LinkedSummary.Paired(device?.serverName ?: state.pc.serverName, state.reachable)
            is LinkState.Failed -> LinkedSummary.Problem
            LinkState.Pairing, LinkState.Unpaired -> LinkedSummary.None
        }
    }

    private val overview: Flow<LoadState<YouOverview>> = combine(
        combine(lists.accounts, history.lastSyncedAt) { accounts, synced -> accounts to synced },
        historyCount,
        extensions.installed,
        torrentFacts,
        linked,
    ) { (accounts, synced), count, installed, (active, total), linkedSummary ->
        when (count) {
            LoadState.Loading -> LoadState.Loading
            is LoadState.Err -> count
            is LoadState.Ok -> LoadState.Ok(
                YouOverview(
                    accounts = accounts,
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

    /** "Sincronizar ahora": push dirty history rows to the paired PC and pull its recent ones; the result goes to the global snackbar. */
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
