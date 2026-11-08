// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.link

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.extensionserver.DeepLink
import app.tatakai.next.extensionserver.DeepLinkResult
import app.tatakai.next.extensionserver.ExtensionServerConnection
import app.tatakai.next.extensionserver.ExtensionServerException
import app.tatakai.next.extensionserver.ExtensionServerRepository
import app.tatakai.next.extensionserver.HostGuard
import app.tatakai.next.extensionserver.PairingLink
import app.tatakai.next.extensionserver.PendingServerLink
import app.tatakai.next.extensionserver.ServerStatus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.net.URI

/** One linked server as the screen shows it. */
data class ServerRow(val connection: ExtensionServerConnection, val status: ServerStatus)

/** What went wrong, mapped to `extserver_err_*` strings by the screen. Never carries the server's message text. */
sealed interface ExtServerError {
    data object InvalidLink : ExtServerError
    data class InsecureHost(val host: String?) : ExtServerError
    data object InvalidUrl : ExtServerError
    data object ExpiredLink : ExtServerError
    data object Unreachable : ExtServerError
    data object WrongPassword : ExtServerError
    data object PairingRequired : ExtServerError
    data object Incompatible : ExtServerError
    data object SetupRequired : ExtServerError
    data object Throttled : ExtServerError
    data class Other(val code: String) : ExtServerError
}

data class ExtensionServersUiState(
    val loaded: Boolean = false,
    val servers: List<ServerRow> = emptyList(),
    val connecting: Boolean = false,
    val error: ExtServerError? = null,
    /** A link opened from outside the app, waiting for the user's confirmation (an opened link never pairs on its own). */
    val pendingLink: PairingLink? = null,
)

sealed interface ExtensionServersEvent {
    data class Connected(val name: String) : ExtensionServersEvent
}

/**
 * "PC extension server" screen (docs/redesign/13 §3.10): pair by QR / pasted link / opened deep link, or URL + viewer password; list,
 * enable / disable and remove linked servers; reachability check on open.
 */
class ExtensionServersViewModel(
    private val repository: ExtensionServerRepository,
    private val pending: PendingServerLink,
) : ViewModel() {
    private val _state = MutableStateFlow(ExtensionServersUiState())
    val state: StateFlow<ExtensionServersUiState> = _state.asStateFlow()

    private val _events = Channel<ExtensionServersEvent>(Channel.BUFFERED)
    val events: Flow<ExtensionServersEvent> = _events.receiveAsFlow()

    init {
        viewModelScope.launch {
            combine(repository.loaded, repository.servers, repository.status) { loaded, servers, status ->
                Triple(loaded, servers, status)
            }.collect { (loaded, servers, status) ->
                _state.update { s ->
                    s.copy(loaded = loaded, servers = servers.map { ServerRow(it, status[it.id] ?: ServerStatus.Unknown) })
                }
            }
        }
        // Deep links opened while the app runs (onNewIntent) arrive here too.
        viewModelScope.launch {
            pending.link.collect { raw -> if (raw != null) pending.consume()?.let(::offerOpenedLink) }
        }
        viewModelScope.launch { runCatchingNonCancel { repository.refresh() } }
    }

    /** A link opened from outside: validated now, paired only after [confirmPending]. */
    fun offerOpenedLink(raw: String) {
        when (val r = DeepLink.parseResult(raw)) {
            is DeepLinkResult.Valid -> _state.update { it.copy(pendingLink = r.link, error = null) }
            is DeepLinkResult.InsecureHost -> _state.update { it.copy(error = ExtServerError.InsecureHost(r.host)) }
            DeepLinkResult.Invalid -> _state.update { it.copy(error = ExtServerError.InvalidLink) }
        }
    }

    fun confirmPending() {
        val link = _state.value.pendingLink ?: return
        _state.update { it.copy(pendingLink = null) }
        connect { repository.connectWithLink(link) }
    }

    fun dismissPending() {
        _state.update { it.copy(pendingLink = null) }
    }

    /** Text from the QR scanner or the paste dialog (the user acted, so it pairs right away). */
    fun connectWithText(text: String) {
        when (val r = DeepLink.parseResult(text)) {
            is DeepLinkResult.Valid -> connect { repository.connectWithLink(r.link) }
            is DeepLinkResult.InsecureHost -> _state.update { it.copy(error = ExtServerError.InsecureHost(r.host)) }
            DeepLinkResult.Invalid -> _state.update { it.copy(error = ExtServerError.InvalidLink) }
        }
    }

    fun connectManual(url: String, password: String) {
        val origin = HostGuard.originOf(url)
        if (origin == null) {
            _state.update { it.copy(error = ExtServerError.InvalidUrl) }
            return
        }
        if (!HostGuard.isAllowed(origin)) {
            _state.update { it.copy(error = ExtServerError.InsecureHost(hostOf(origin))) }
            return
        }
        connect { repository.connectWithPassword(origin, password) }
    }

    /** Pure validation for the paste dialog. */
    fun isPlausibleLink(text: String): Boolean = DeepLink.parseResult(text) !is DeepLinkResult.Invalid

    fun setEnabled(id: String, enabled: Boolean) {
        viewModelScope.launch {
            runCatchingNonCancel { repository.setEnabled(id, enabled) }
            if (enabled) runCatchingNonCancel { repository.refresh(id) }
        }
    }

    fun remove(id: String) {
        viewModelScope.launch { runCatchingNonCancel { repository.remove(id) } }
    }

    fun refresh() {
        viewModelScope.launch { runCatchingNonCancel { repository.refresh() } }
    }

    fun clearError() {
        _state.update { it.copy(error = null) }
    }

    private fun connect(block: suspend () -> ExtensionServerConnection) {
        if (_state.value.connecting) return
        _state.update { it.copy(connecting = true, error = null) }
        viewModelScope.launch {
            try {
                val conn = block()
                _state.update { it.copy(connecting = false) }
                _events.trySend(ExtensionServersEvent.Connected(conn.name))
                runCatchingNonCancel { repository.refresh(conn.id) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _state.update { it.copy(connecting = false, error = errorOf(e)) }
            }
        }
    }

    private suspend fun runCatchingNonCancel(block: suspend () -> Unit) {
        try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            // Reflected through repository.status; nothing else to show.
        }
    }

    internal companion object {
        fun errorOf(e: Throwable): ExtServerError {
            val x = e as? ExtensionServerException ?: return ExtServerError.Unreachable
            return when (x.code) {
                ExtensionServerException.INVALID_LINK -> ExtServerError.InvalidLink
                ExtensionServerException.INSECURE_HOST -> ExtServerError.InsecureHost(null)
                ExtensionServerException.INVALID_URL -> ExtServerError.InvalidUrl
                ExtensionServerException.PAIRING_EXPIRED -> ExtServerError.ExpiredLink
                ExtensionServerException.UNREACHABLE -> ExtServerError.Unreachable
                ExtensionServerException.INVALID_PASSWORD -> ExtServerError.WrongPassword
                ExtensionServerException.PAIRING_REQUIRED, ExtensionServerException.AUTHENTICATION_REQUIRED -> ExtServerError.PairingRequired
                ExtensionServerException.INCOMPATIBLE, ExtensionServerException.BAD_RESPONSE -> ExtServerError.Incompatible
                ExtensionServerException.SETUP_REQUIRED -> ExtServerError.SetupRequired
                ExtensionServerException.LOGIN_THROTTLED -> ExtServerError.Throttled
                else -> ExtServerError.Other(x.code)
            }
        }

        private fun hostOf(origin: String): String? = runCatching { URI(origin).host }.getOrNull()
    }
}
