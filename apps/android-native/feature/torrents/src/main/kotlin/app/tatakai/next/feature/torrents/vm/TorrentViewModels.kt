// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.vm

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.model.torrent.TorrentSettings
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.model.torrent.TorrentStorage
import app.tatakai.next.model.torrent.TorrentTracker
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onStart
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

// ViewModels of the torrent client (docs/redesign/11 §3, 12 §3.12). Data only through TorrentRepository / SettingsRepository (Koin).

/** One-shot messages for the global snackbar (translated by the screens). */
sealed interface TorrentMessage {
    data class Removed(val count: Int) : TorrentMessage
    data class Exported(val uri: String) : TorrentMessage
    data class Failed(val error: AppError) : TorrentMessage
    data object Saved : TorrentMessage
}

private fun messages() = MutableSharedFlow<TorrentMessage>(extraBufferCapacity = 8, onBufferOverflow = BufferOverflow.DROP_OLDEST)

/** Wraps a flow into Loading / Ok / Err, restarting on [reload]. */
@OptIn(ExperimentalCoroutinesApi::class)
internal fun <T> loadStateOf(reload: Flow<Int>, source: () -> Flow<T>): Flow<LoadState<T>> = reload.flatMapLatest {
    source()
        .map<T, LoadState<T>> { LoadState.Ok(it) }
        .onStart { emit(LoadState.Loading) }
        .catch { e ->
            if (e is CancellationException) throw e
            emit(LoadState.Err(e.toAppError()))
        }
}

/** Polls a suspend getter every [periodMs] while collected. */
internal fun <T> polling(periodMs: Long, get: suspend () -> T): Flow<T> = flow {
    while (true) {
        emit(get())
        delay(periodMs)
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Library (11 §3.2)
// ---------------------------------------------------------------------------------------------------------------------------------

class TorrentLibraryViewModel(private val repository: TorrentRepository) : ViewModel() {

    private val reload = MutableStateFlow(0)

    val state: StateFlow<LoadState<List<TorrentLibraryItem>>> = loadStateOf(reload) { repository.library }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    val status: StateFlow<TorrentServiceStatus> = repository.status

    private val _selection = MutableStateFlow<Set<String>>(emptySet())

    /** Multi-select (long press) ids; empty = normal mode. */
    val selection: StateFlow<Set<String>> = _selection.asStateFlow()

    private val _messages = messages()
    val messages: SharedFlow<TorrentMessage> = _messages.asSharedFlow()

    fun retry() {
        reload.value += 1
    }

    fun toggleSelection(id: String) {
        _selection.value = _selection.value.let { if (id in it) it - id else it + id }
    }

    fun selectAll(ids: Collection<String>) {
        _selection.value = ids.toSet()
    }

    fun clearSelection() {
        _selection.value = emptySet()
    }

    fun pause(ids: Collection<String>) = act { ids.forEach { repository.pause(it) } }

    fun resume(ids: Collection<String>) = act { ids.forEach { repository.resume(it) } }

    fun setPersist(ids: Collection<String>, persist: Boolean) = act { ids.forEach { repository.setPersist(it, persist) } }

    fun remove(ids: Collection<String>, deleteFiles: Boolean) = act {
        ids.forEach { repository.remove(it, deleteFiles) }
        _selection.value = _selection.value - ids.toSet()
        _messages.tryEmit(TorrentMessage.Removed(ids.size))
    }

    private fun act(block: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _messages.tryEmit(TorrentMessage.Failed(e.toAppError()))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Detail: Overview / Files / Peers / Trackers (11 §3.3)
// ---------------------------------------------------------------------------------------------------------------------------------

/** Sort keys of the Peers tab (SingleChoiceSegmentedButtonRow ↓ / ↑ / progress). */
enum class PeerSort { DOWN, UP, PROGRESS }

/** Peers ordered by [sort], fastest / most complete first; ties keep the engine order. */
fun sortPeers(peers: List<TorrentPeer>, sort: PeerSort): List<TorrentPeer> = when (sort) {
    PeerSort.DOWN -> peers.sortedByDescending { it.downBps }
    PeerSort.UP -> peers.sortedByDescending { it.upBps }
    PeerSort.PROGRESS -> peers.sortedByDescending { it.progress }
}

/** Files whose name or folder contains [query] (case-insensitive); blank = all. */
fun filterFiles(files: List<TorrentFile>, query: String): List<TorrentFile> {
    val q = query.trim()
    if (q.isEmpty()) return files
    return files.filter { it.path.contains(q, ignoreCase = true) }
}

class TorrentDetailViewModel(
    val torrentId: String,
    private val repository: TorrentRepository,
) : ViewModel() {

    private val reloadStats = MutableStateFlow(0)
    private val reloadFiles = MutableStateFlow(0)
    private val reloadPeers = MutableStateFlow(0)
    private val reloadTrackers = MutableStateFlow(0)

    val stats: StateFlow<LoadState<TorrentStats>> = loadStateOf(reloadStats) { repository.stats(torrentId) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    val files: StateFlow<LoadState<List<TorrentFile>>> = loadStateOf(reloadFiles) { polling(FILES_POLL_MS) { repository.files(torrentId) } }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    val peers: StateFlow<LoadState<List<TorrentPeer>>> = loadStateOf(reloadPeers) { repository.peers(torrentId) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    val trackers: StateFlow<LoadState<List<TorrentTracker>>> =
        loadStateOf(reloadTrackers) { polling(TRACKERS_POLL_MS) { repository.trackers(torrentId) } }
            .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    /** The library row of this torrent (label, persist, seed), null when it left the library. */
    val item: StateFlow<TorrentLibraryItem?> = repository.library
        .map { list -> list.firstOrNull { it.id == torrentId } }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), repository.library.value.firstOrNull { it.id == torrentId })

    private val _messages = messages()
    val messages: SharedFlow<TorrentMessage> = _messages.asSharedFlow()

    private val _removed = MutableStateFlow(false)

    /** true after a successful remove: the screen navigates back. */
    val removed: StateFlow<Boolean> = _removed.asStateFlow()

    fun retryStats() {
        reloadStats.value += 1
    }

    fun retryFiles() {
        reloadFiles.value += 1
    }

    fun retryPeers() {
        reloadPeers.value += 1
    }

    fun retryTrackers() {
        reloadTrackers.value += 1
    }

    fun pause() = act { repository.pause(torrentId) }

    fun resume() = act { repository.resume(torrentId) }

    fun setPersist(persist: Boolean) = act { repository.setPersist(torrentId, persist) }

    fun setSeeding(seed: Boolean) = act { repository.setSeeding(torrentId, seed) }

    fun downloadFile(index: Int) = act {
        repository.downloadFile(torrentId, index)
        reloadFiles.value += 1
    }

    fun export(index: Int) = act {
        repository.exportToDownloads(torrentId, index)
            .onSuccess { _messages.tryEmit(TorrentMessage.Exported(it)) }
            .onFailure { _messages.tryEmit(TorrentMessage.Failed(it.toAppError())) }
    }

    fun remove(deleteFiles: Boolean) = act {
        repository.remove(torrentId, deleteFiles)
        _messages.tryEmit(TorrentMessage.Removed(1))
        _removed.value = true
    }

    private fun act(block: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _messages.tryEmit(TorrentMessage.Failed(e.toAppError()))
            }
        }
    }

    private companion object {
        const val FILES_POLL_MS = 2_000L
        const val TRACKERS_POLL_MS = 5_000L
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Downloads (11 §3.4)
// ---------------------------------------------------------------------------------------------------------------------------------

/** Downloads list: background / persisted torrents not complete yet, newest first (the library order). */
fun downloadsOf(library: List<TorrentLibraryItem>): List<TorrentLibraryItem> =
    library.filter { it.persist && it.progress < 1.0 }

data class DownloadsUi(
    val settings: TorrentSettings,
    val items: List<TorrentLibraryItem>,
    val status: TorrentServiceStatus,
)

class TorrentDownloadsViewModel(
    private val repository: TorrentRepository,
    private val settingsRepository: SettingsRepository,
) : ViewModel() {

    private val reload = MutableStateFlow(0)

    val state: StateFlow<LoadState<DownloadsUi>> = loadStateOf(reload) {
        kotlinx.coroutines.flow.combine(settingsRepository.settings, repository.library, repository.status) { s, lib, st ->
            DownloadsUi(settings = s.torrent, items = downloadsOf(lib), status = st)
        }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    private val _messages = messages()
    val messages: SharedFlow<TorrentMessage> = _messages.asSharedFlow()

    fun retry() {
        reload.value += 1
    }

    /** "Entiendo": stores the ack and turns background downloads on (12 §3.12 DownloadGate semantics). */
    fun acknowledge() = act {
        settingsRepository.acknowledgeBackgroundDownloads()
        val t = settingsRepository.settings.value.torrent
        settingsRepository.setTorrentSettings(t.copy(backgroundDownloads = true))
    }

    fun setBackground(enabled: Boolean) = act {
        val t = settingsRepository.settings.value.torrent
        settingsRepository.setTorrentSettings(t.copy(backgroundDownloads = enabled))
    }

    fun pause(id: String) = act { repository.pause(id) }

    fun resume(id: String) = act { repository.resume(id) }

    fun remove(id: String) = act {
        repository.remove(id, deleteFiles = true)
        _messages.tryEmit(TorrentMessage.Removed(1))
    }

    private fun act(block: suspend () -> Unit) {
        viewModelScope.launch {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _messages.tryEmit(TorrentMessage.Failed(e.toAppError()))
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Settings page (11 §3.5)
// ---------------------------------------------------------------------------------------------------------------------------------

/** Inline validation of the numeric fields (desktop `torrent.settings.*_invalid`). */
object TorrentFieldRules {
    /** 0 or 1024..=65535; null = invalid. */
    fun port(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it == 0 || it in 1024..65535 }

    /** 0..=MAX_LIMIT_KIB (0 = unlimited); null = invalid. */
    fun limit(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it in 0..TorrentSettings.MAX_LIMIT_KIB }

    /** 1..=MAX_PEERS; null = invalid. */
    fun peers(text: String): Int? = text.trim().toIntOrNull()?.takeIf { it in 1..TorrentSettings.MAX_PEERS }
}

class TorrentSettingsViewModel(
    private val settingsRepository: SettingsRepository,
    private val repository: TorrentRepository,
) : ViewModel() {

    val settings: StateFlow<TorrentSettings> = settingsRepository.settings
        .map { it.torrent }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), settingsRepository.settings.value.torrent)

    private val reloadStorage = MutableStateFlow(0)

    val storage: StateFlow<LoadState<TorrentStorage>> = loadStateOf(reloadStorage) { flow { emit(repository.storageUsage()) } }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    private val _messages = messages()
    val messages: SharedFlow<TorrentMessage> = _messages.asSharedFlow()

    fun retryStorage() {
        reloadStorage.value += 1
    }

    /** Immediate save of one field (DataStore); `setTorrentSettings` sanitizes. */
    fun update(transform: (TorrentSettings) -> TorrentSettings) {
        viewModelScope.launch {
            try {
                val current = settingsRepository.settings.value.torrent
                val next = transform(current)
                if (next != current) {
                    settingsRepository.setTorrentSettings(next)
                    _messages.tryEmit(TorrentMessage.Saved)
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _messages.tryEmit(TorrentMessage.Failed(e.toAppError()))
            }
        }
    }
}
