package app.tatakai.next.feature.link

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.link.LinkDiscoveryAccess
import app.tatakai.next.model.AppError
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkHandoff
import app.tatakai.next.model.RemoteAction
import app.tatakai.next.model.RemotePlayerState
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LinkedDevice
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class LinkUiState(
    val link: LinkState = LinkState.Unpaired,
    /** Null until a PC is paired. */
    val capabilities: LoadState<LinkCapabilities>? = null,
    /** Profile id chosen by the user; null = the PC's default profile. */
    val selectedProfile: String? = null,
    val pairing: Boolean = false,
    val pairError: AppError? = null,
    val startingSession: Boolean = false,
    val sessionError: AppError? = null,
    // --- android wave (12 §3.13): "Dispositivo enlazado" ---
    /** The paired PC as persisted by the Link client (fingerprint, last seen, preferred profile); null = unpaired. */
    val device: LinkedDevice? = null,
    // --- parity wave B (16 §3.11, worker r) ---
    /** `GET /v1/player` (polled every 2 s while the screen is visible); null = nothing playing / no remote control on the PC. */
    val remote: RemotePlayerState? = null,
    val remoteBusy: Boolean = false,
    /** "Continuar en la PC": the newest history row of this phone (null = nothing to hand over). */
    val continueEntry: HistoryEntry? = null,
    val handingOff: Boolean = false,
    /** PCs announced over mDNS (link.discovery). */
    val discovered: List<DiscoveredPc> = emptyList(),
    /** PC -> phone "Enviar a dispositivo" waiting for "Reproducir aquí". */
    val pendingHandoff: LinkHandoff? = null,
)

sealed interface LinkEvent {
    data class NavigateToPlayer(val args: PlayerArgs) : LinkEvent

    /** parity wave B: one-shot results for the snackbar. */
    data object HandedOff : LinkEvent
    data class Failed(val error: AppError) : LinkEvent
}

class LinkViewModel(
    private val link: LinkRepository,
    private val deviceName: String,
    /** parity wave B: "Continuar en la PC" pushes this history first (`syncNow`) and hands over its newest row. */
    private val history: HistoryRepository? = null,
    /** parity wave B: pairing with a PC found over mDNS (fingerprint + one-time code). */
    private val discovery: LinkDiscoveryAccess? = null,
) : ViewModel() {

    private val _state = MutableStateFlow(
        LinkUiState(link = link.state.value, device = link.linkedDevice.value, selectedProfile = link.linkedDevice.value?.preferredProfile),
    )
    val state: StateFlow<LinkUiState> = _state.asStateFlow()

    private val _events = Channel<LinkEvent>(Channel.BUFFERED)
    val events: Flow<LinkEvent> = _events.receiveAsFlow()

    init {
        viewModelScope.launch {
            link.state.collectLatest { s ->
                _state.update {
                    it.copy(
                        link = s,
                        capabilities = if (s is LinkState.Paired) it.capabilities else null,
                        selectedProfile = if (s is LinkState.Unpaired) null else it.selectedProfile,
                    )
                }
                if (s is LinkState.Paired) loadCapabilities()
            }
        }
        viewModelScope.launch {
            link.linkedDevice.collect { device ->
                _state.update { it.copy(device = device, selectedProfile = it.selectedProfile ?: device?.preferredProfile) }
            }
        }
        viewModelScope.launch {
            link.pendingHandoff.collect { h -> _state.update { it.copy(pendingHandoff = h) } }
        }
        history?.let { h ->
            viewModelScope.launch {
                h.continueWatching(1).catch { emit(emptyList()) }.collect { rows -> _state.update { it.copy(continueEntry = rows.firstOrNull()) } }
            }
        }
        viewModelScope.launch {
            link.discover().catch { emit(emptyList()) }.collect { pcs -> _state.update { it.copy(discovered = pcs) } }
        }
    }

    // ------------------------------------------------------------------ parity wave B (16 §3.11)

    /** Polls `GET /v1/player` every [REMOTE_POLL_MS] while the screen is composed (the screen runs this in a LaunchedEffect). */
    suspend fun pollRemote() {
        while (true) {
            if (_state.value.link is LinkState.Paired) {
                val r = runCatchingApp { link.remoteState().getOrThrow() }
                _state.update { it.copy(remote = r.getOrNull()) }
            }
            delay(REMOTE_POLL_MS)
        }
    }

    fun remote(action: RemoteAction) {
        if (_state.value.remoteBusy) return
        _state.update { it.copy(remoteBusy = true) }
        viewModelScope.launch {
            val r = runCatchingApp { link.remote(action).getOrThrow() }
            r.onFailure { _events.trySend(LinkEvent.Failed(it.toAppError())) }
            val st = runCatchingApp { link.remoteState().getOrThrow() }.getOrNull()
            _state.update { it.copy(remoteBusy = false, remote = st ?: it.remote) }
        }
    }

    /** "Continuar en la PC": push the history (so the PC has the row), then `open` at the phone's position. */
    fun continueOnPc() {
        val entry = _state.value.continueEntry ?: return
        if (_state.value.handingOff) return
        _state.update { it.copy(handingOff = true) }
        viewModelScope.launch {
            runCatchingApp { history?.syncNow() }
            val r = runCatchingApp { link.handoff(entry.animeId, entry.episode, entry.progressSec).getOrThrow() }
            _state.update { it.copy(handingOff = false) }
            r.fold(onSuccess = { _events.trySend(LinkEvent.HandedOff) }, onFailure = { _events.trySend(LinkEvent.Failed(it.toAppError())) })
        }
    }

    /** "Reproducir aquí" of a PC -> phone hand-off: the PC already started the Link session. */
    fun playHandoff() {
        val h = _state.value.pendingHandoff ?: return
        link.dismissHandoff()
        _events.trySend(LinkEvent.NavigateToPlayer(LinkLogic.handoffArgs(h)))
    }

    fun dismissHandoff() = link.dismissHandoff()

    fun fingerprintOf(pc: DiscoveredPc): String? = discovery?.fingerprintOf(pc)

    val canPairDiscovered: Boolean get() = discovery != null

    /** Pairs with a PC found on the network after the user confirmed its fingerprint and typed the code shown on it. */
    fun pairDiscovered(pc: DiscoveredPc, code: String) {
        val d = discovery ?: return
        if (_state.value.pairing) return
        _state.update { it.copy(pairing = true, pairError = null) }
        viewModelScope.launch {
            val result = runCatchingApp { d.pairDiscovered(pc, code, deviceName).getOrThrow() }
            _state.update { it.copy(pairing = false, pairError = result.exceptionOrNull()?.toAppError()) }
        }
    }

    /** Validates a pasted pairing text before sending it (prototype: `tatakai-link:v1?…` or a 10-character code). */
    fun isValidPairingText(text: String): Boolean = PAIRING_TEXT.matches(text.trim())

    /** [payload] is the `tatakai-link:v1` URI read from the QR code or pasted by the user. */
    fun pair(payload: String) {
        val text = payload.trim()
        if (text.isEmpty() || _state.value.pairing) return
        _state.update { it.copy(pairing = true, pairError = null) }
        viewModelScope.launch {
            val result = runCatchingApp { link.pair(text, deviceName).getOrThrow() }
            _state.update { s ->
                s.copy(pairing = false, pairError = result.exceptionOrNull()?.toAppError())
            }
        }
    }

    fun retryCapabilities() {
        viewModelScope.launch { loadCapabilities() }
    }

    private suspend fun loadCapabilities() {
        _state.update { it.copy(capabilities = LoadState.Loading) }
        val result = runCatchingApp { link.capabilities().getOrThrow() }
        _state.update { s ->
            val caps = result.fold(
                onSuccess = { LoadState.Ok(it) },
                onFailure = { LoadState.Err(it.toAppError()) },
            )
            val wanted = s.selectedProfile ?: s.device?.preferredProfile
            val keep = wanted?.takeIf { id -> (caps as? LoadState.Ok)?.value?.profiles?.any { it.id == id } == true }
            s.copy(capabilities = caps, selectedProfile = keep)
        }
    }

    /** Enhance profile segmented: kept in the UI state and persisted as the PC's preferred profile for this phone. */
    fun selectProfile(id: String?) {
        _state.update { it.copy(selectedProfile = id) }
        viewModelScope.launch { runCatchingApp { link.setPreferredProfile(id) } }
    }

    /** Starts a remote (PC-transcoded) session for [url] and opens the player on it. */
    fun startSession(url: String) {
        val trimmed = url.trim()
        if (trimmed.isEmpty() || _state.value.startingSession) return
        _state.update { it.copy(startingSession = true, sessionError = null) }
        viewModelScope.launch {
            val profile = _state.value.selectedProfile
            val result = runCatchingApp { link.startSession(LinkSource.Url(trimmed), profile).getOrThrow() }
            result.onSuccess { session ->
                _state.update { it.copy(startingSession = false) }
                _events.trySend(
                    LinkEvent.NavigateToPlayer(
                        PlayerArgs(media = MediaRef(), title = trimmed, episode = 1, remoteSessionId = session.sessionId),
                    ),
                )
            }.onFailure { e ->
                _state.update { it.copy(startingSession = false, sessionError = e.toAppError()) }
            }
        }
    }

    fun forget() {
        viewModelScope.launch { link.forget() }
    }

    private companion object {
        const val REMOTE_POLL_MS = 2_000L
        val PAIRING_TEXT = Regex("^(tatakai-link:v1\\?.+|[0-9A-Z]{10})$", RegexOption.IGNORE_CASE)
    }
}

/** Pure helpers of the Linked screen (JVM-tested). */
internal object LinkLogic {
    /** Player args of a PC -> phone hand-off: the Link session, from the PC's position. */
    fun handoffArgs(h: LinkHandoff): PlayerArgs = PlayerArgs(
        media = h.animeId?.let(::refOf) ?: MediaRef(),
        title = h.title ?: "",
        episode = h.episode ?: 1,
        startMs = h.positionSec * 1000L,
        remoteSessionId = h.sessionId,
    )

    /** `HistoryEntry.animeId` (`"21"` / `"mal:5"`) -> ref. */
    fun refOf(animeId: String): MediaRef? = when {
        animeId.startsWith("mal:") -> animeId.removePrefix("mal:").toIntOrNull()?.let { MediaRef(malId = it) }
        else -> animeId.toIntOrNull()?.let { MediaRef(anilistId = it) }
    }

    /** "12:05 / 24:00" of the remote card (durations unknown -> position only). */
    fun clock(positionSec: Int, durationSec: Int?): String {
        fun fmt(s: Int): String = if (s >= 3600) "%d:%02d:%02d".format(s / 3600, s / 60 % 60, s % 60) else "%d:%02d".format(s / 60, s % 60)
        return if (durationSec != null) "${fmt(positionSec)} / ${fmt(durationSec)}" else fmt(positionSec)
    }

    /** PCs to list: the paired one (same host and port) is not offered again. */
    fun others(found: List<DiscoveredPc>, pairedHost: String?, pairedPort: Int?): List<DiscoveredPc> =
        found.filterNot { it.host == pairedHost && it.port == pairedPort }.distinctBy { it.instanceId ?: "${it.host}:${it.port}" }
}
