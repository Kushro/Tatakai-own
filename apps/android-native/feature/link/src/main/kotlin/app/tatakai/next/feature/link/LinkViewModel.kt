package app.tatakai.next.feature.link

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
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
)

sealed interface LinkEvent {
    data class NavigateToPlayer(val args: PlayerArgs) : LinkEvent
}

class LinkViewModel(
    private val link: LinkRepository,
    private val deviceName: String,
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
        val PAIRING_TEXT = Regex("^(tatakai-link:v1\\?.+|[0-9A-Z]{10})$", RegexOption.IGNORE_CASE)
    }
}
