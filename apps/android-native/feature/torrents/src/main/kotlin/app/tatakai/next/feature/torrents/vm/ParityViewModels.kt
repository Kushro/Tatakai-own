// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.vm

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.torrent.DebridAccount
import app.tatakai.next.model.torrent.DebridService
import app.tatakai.next.model.torrent.EpisodeAlignment
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentSessionEntry
import app.tatakai.next.torrent.PendingMagnetLink
import app.tatakai.next.torrent.debrid.DebridRepositoryImpl
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.util.Locale

// parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, worker q): "Añadir torrent" (torrent.add-magnet,
// torrent.add-file, torrent.magnet-link-handler, torrent.episode-alignment), session history (torrent.session-history) and the
// Debrid section (sources.debrid.settings).

/** What the "Añadir torrent" field holds: a magnet, a 40-hex info-hash, or a `.torrent` picked with SAF. */
enum class AddSourceKind { MAGNET, HASH, FILE, INVALID, EMPTY }

private val HEX40 = Regex("[0-9a-fA-F]{40}")

/** Classifies [text]; a bare hash, `magnet:?xt=urn:btih:…` and `content://` / `file://` URIs are accepted. Pure, unit tested. */
fun classifyAddSource(text: String): AddSourceKind {
    val t = text.trim()
    return when {
        t.isEmpty() -> AddSourceKind.EMPTY
        t.startsWith("magnet:", ignoreCase = true) && t.contains("xt=urn:btih:", ignoreCase = true) -> AddSourceKind.MAGNET
        HEX40.matches(t) -> AddSourceKind.HASH
        t.startsWith("content://") || t.startsWith("file://") -> AddSourceKind.FILE
        else -> AddSourceKind.INVALID
    }
}

data class AddTorrentUi(
    val source: String = "",
    /** Display name of a picked `.torrent` (the source then holds its URI). */
    val fileName: String? = null,
    val kind: AddSourceKind = AddSourceKind.EMPTY,
    val persist: Boolean = true,
    val seriesQuery: String = "",
    val results: LoadState<List<Media>>? = null,
    val media: Media? = null,
    val firstEpisode: Int = 1,
    val offset: Int = 0,
    val adding: Boolean = false,
    val error: AppError? = null,
)

sealed interface AddTorrentEvent {
    data class Added(val id: String) : AddTorrentEvent
}

class AddTorrentViewModel(
    private val repository: TorrentRepository,
    private val metadata: MetadataRepository? = null,
    private val pending: PendingMagnetLink? = null,
) : ViewModel() {

    private val _state = MutableStateFlow(AddTorrentUi())
    val state: StateFlow<AddTorrentUi> = _state.asStateFlow()
    private val _events = Channel<AddTorrentEvent>(Channel.BUFFERED)
    val events: Flow<AddTorrentEvent> = _events.receiveAsFlow()

    /** A link handed over by the system (`magnet:` / `.torrent` intent): the screen opens the sheet when this turns true. */
    private val _incoming = MutableStateFlow(false)
    val incoming: StateFlow<Boolean> = _incoming.asStateFlow()
    private var searchJob: Job? = null

    init {
        pending?.let { p ->
            viewModelScope.launch {
                p.link.filterNotNull().collect {
                    val raw = p.consume() ?: return@collect
                    reset()
                    setSource(raw, fileName = if (raw.startsWith("content://") || raw.startsWith("file://")) raw.substringAfterLast('/') else null)
                    _incoming.value = true
                }
            }
        }
    }

    fun incomingShown() {
        _incoming.value = false
    }

    fun reset() {
        searchJob?.cancel()
        _state.value = AddTorrentUi()
    }

    fun setSource(text: String, fileName: String? = null) =
        _state.update { it.copy(source = text, fileName = fileName, kind = classifyAddSource(text), error = null) }

    fun setPersist(on: Boolean) = _state.update { it.copy(persist = on) }

    fun setFirstEpisode(value: Int) = _state.update { it.copy(firstEpisode = value.coerceIn(1, 9_999)) }

    fun setOffset(value: Int) = _state.update { it.copy(offset = value.coerceIn(-9_999, 9_999)) }

    fun pickMedia(media: Media?) = _state.update { it.copy(media = media, results = null, seriesQuery = media?.title ?: it.seriesQuery) }

    /** Debounced series search (AniList through MetadataRepository). */
    fun setSeriesQuery(query: String) {
        _state.update { it.copy(seriesQuery = query, media = null) }
        searchJob?.cancel()
        val repo = metadata ?: return
        if (query.trim().length < 2) {
            _state.update { it.copy(results = null) }
            return
        }
        searchJob = viewModelScope.launch {
            delay(SEARCH_DEBOUNCE_MS)
            _state.update { it.copy(results = LoadState.Loading) }
            val result = runCatchingApp { repo.search(MediaKind.ANIME, query.trim()).items.take(MAX_RESULTS) }
            _state.update { s -> s.copy(results = result.fold(onSuccess = { LoadState.Ok(it) }, onFailure = { LoadState.Err(it.toAppError()) })) }
        }
    }

    fun add() {
        val s = _state.value
        if (s.adding || s.kind == AddSourceKind.EMPTY || s.kind == AddSourceKind.INVALID) return
        _state.update { it.copy(adding = true, error = null) }
        viewModelScope.launch {
            val alignment = if (s.media != null || s.firstEpisode != 1 || s.offset != 0) EpisodeAlignment(s.firstEpisode, s.offset) else null
            val source = if (s.kind == AddSourceKind.HASH) s.source.trim().lowercase(Locale.ROOT) else s.source.trim()
            val result = runCatchingApp { repository.addManual(source, s.media, alignment, s.persist).getOrThrow() }
            result.fold(
                onSuccess = { id ->
                    _state.value = AddTorrentUi()
                    _events.trySend(AddTorrentEvent.Added(id))
                },
                onFailure = { e -> _state.update { it.copy(adding = false, error = e.toAppError()) } },
            )
        }
    }

    private companion object {
        const val SEARCH_DEBOUNCE_MS = 350L
        const val MAX_RESULTS = 8
    }
}

// ------------------------------------------------------------------------------------------------ session history

class TorrentHistoryViewModel(private val repository: TorrentRepository) : ViewModel() {
    private val reload = MutableStateFlow(0)

    val state: StateFlow<LoadState<List<TorrentSessionEntry>>> = loadStateOf(reload) { repository.sessionHistory() }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    /** Ids still in the library (open them) vs gone (re-add them first). */
    val libraryIds: StateFlow<Set<String>> = repository.library.map { lib -> lib.map { it.id }.toSet() }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptySet())

    private val _events = Channel<TorrentHistoryEvent>(Channel.BUFFERED)
    val events: Flow<TorrentHistoryEvent> = _events.receiveAsFlow()

    fun retry() {
        reload.value += 1
    }

    fun clear() {
        viewModelScope.launch {
            runCatchingApp { repository.clearSessionHistory() }.onFailure { _events.trySend(TorrentHistoryEvent.Failed(it.toAppError())) }
        }
    }

    /** "Volver a abrir": a removed torrent is added again (streamed, not kept) by its info-hash, then opened. */
    fun reopen(entry: TorrentSessionEntry) {
        if (entry.id in libraryIds.value) {
            _events.trySend(TorrentHistoryEvent.Open(entry.id))
            return
        }
        viewModelScope.launch {
            runCatchingApp { repository.addManual(entry.id, media = null, alignment = null, persist = false).getOrThrow() }.fold(
                onSuccess = { _events.trySend(TorrentHistoryEvent.Open(it)) },
                onFailure = { _events.trySend(TorrentHistoryEvent.Failed(it.toAppError())) },
            )
        }
    }
}

sealed interface TorrentHistoryEvent {
    data class Open(val id: String) : TorrentHistoryEvent
    data class Failed(val error: AppError) : TorrentHistoryEvent
}

// ------------------------------------------------------------------------------------------------ debrid

data class DebridUi(
    val accounts: List<DebridAccount> = emptyList(),
    /** Service whose key is being checked. */
    val linking: DebridService? = null,
    val errors: Map<DebridService, AppError> = emptyMap(),
)

class DebridViewModel(private val debrid: DebridRepositoryImpl?) : ViewModel() {
    private val local = MutableStateFlow(DebridUi())

    val available: Boolean get() = debrid != null

    val state: StateFlow<DebridUi> = combine(debrid?.accounts ?: flowOf(emptyList()), local) { accounts, l -> l.copy(accounts = accounts) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DebridUi())

    private val _events = Channel<DebridAccount>(Channel.BUFFERED)

    /** Linked accounts, for the "Vinculado como …" snackbar. */
    val linked: Flow<DebridAccount> = _events.receiveAsFlow()

    /** "Probar y vincular": validates the key against the service's account endpoint and stores it in the Keystore. */
    fun link(service: DebridService, apiKey: String) {
        val repo = debrid ?: return
        if (local.value.linking != null) return
        local.update { it.copy(linking = service, errors = it.errors - service) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.link(service, apiKey).getOrThrow() }
            local.update { l ->
                l.copy(
                    linking = null,
                    errors = result.exceptionOrNull()?.let { l.errors + (service to it.toAppError()) } ?: l.errors,
                )
            }
            result.getOrNull()?.let { _events.trySend(it) }
        }
    }

    fun unlink(service: DebridService) {
        val repo = debrid ?: return
        viewModelScope.launch { runCatchingApp { repo.unlink(service) } }
    }

    fun prefer(service: DebridService) {
        val repo = debrid ?: return
        viewModelScope.launch { runCatchingApp { repo.prefer(service) } }
    }
}
