// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.extensionserver.ExtensionServerConnection
import app.tatakai.next.extensionserver.ExtensionServerException
import app.tatakai.next.extensionserver.ExtensionServerRepository
import app.tatakai.next.extensionserver.ServerExtension
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// Parity wave B closure (16 §3.4, worker t; checklist server.api.extensions): the extensions installed on each linked PC server
// (`GET /extensions`, or the owner-only fallback rebuilt from `/sources` in ExtensionServerRepositoryImpl.extensions), shown as a section
// of Extensions so the phone sees what the PC offers.

/** Why a server's extension list could not be read (mapped to `extensions_server_err_*`). Never carries server text. */
enum class ServerExtError { UNREACHABLE, UNAUTHORIZED, INCOMPATIBLE, OTHER }

/** The extension list of one server. */
sealed interface ServerExtensionsLoad {
    data object Loading : ServerExtensionsLoad
    data class Ok(val extensions: List<ServerExtension>) : ServerExtensionsLoad
    data class Err(val error: ServerExtError) : ServerExtensionsLoad

    /** The server is turned off on this phone: nothing is requested. */
    data object Disabled : ServerExtensionsLoad
}

data class ServerExtensionsBlock(val id: String, val name: String, val url: String, val load: ServerExtensionsLoad)

data class ServerExtensionsUiState(
    /** False until the linked servers have been read from disk. */
    val loaded: Boolean = false,
    /** False when this build has no extension-server client (the section is hidden). */
    val available: Boolean = false,
    val servers: List<ServerExtensionsBlock> = emptyList(),
)

class ServerExtensionsViewModel(private val repository: ExtensionServerRepository?) : ViewModel() {
    private val loads = MutableStateFlow<Map<String, ServerExtensionsLoad>>(emptyMap())
    private val jobs = mutableMapOf<String, Job>()
    private val _state = MutableStateFlow(ServerExtensionsUiState(available = repository != null))
    val state: StateFlow<ServerExtensionsUiState> = _state.asStateFlow()

    init {
        val repo = repository
        if (repo != null) {
            viewModelScope.launch {
                combine(repo.loaded, repo.servers, loads) { loaded, servers, l -> Triple(loaded, servers, l) }.collect { (loaded, servers, l) ->
                    // A server turned off or removed forgets its list, so turning it back on reads it again.
                    val stale = l.keys - servers.filter { it.enabled }.map { it.id }.toSet()
                    if (stale.isNotEmpty()) {
                        stale.forEach { jobs.remove(it)?.cancel() }
                        loads.update { it - stale }
                    }
                    // Fetch once per enabled server the first time it shows up (refresh() re-reads on demand).
                    servers.filter { it.enabled && it.id !in l && jobs[it.id]?.isActive != true }.forEach { fetch(it.id) }
                    _state.update { it.copy(loaded = loaded, servers = blocksOf(servers, l)) }
                }
            }
        }
    }

    /** Re-reads one server (retry / refresh button), or every enabled server when [id] is null. */
    fun refresh(id: String? = null) {
        val repo = repository ?: return
        repo.servers.value.filter { it.enabled && (id == null || it.id == id) }.forEach { fetch(it.id) }
    }

    private fun fetch(id: String) {
        val repo = repository ?: return
        jobs[id]?.cancel()
        loads.update { it + (id to ServerExtensionsLoad.Loading) }
        jobs[id] = viewModelScope.launch {
            val next = try {
                ServerExtensionsLoad.Ok(sortServerExtensions(repo.extensions(id)))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                ServerExtensionsLoad.Err(serverExtErrorOf(e))
            }
            loads.update { it + (id to next) }
        }
    }

    internal companion object {
        /** Linked servers in their stored order with the state of their list; disabled ones never show a stale list. */
        fun blocksOf(servers: List<ExtensionServerConnection>, loads: Map<String, ServerExtensionsLoad>): List<ServerExtensionsBlock> =
            servers.map { c ->
                val load = if (!c.enabled) ServerExtensionsLoad.Disabled else loads[c.id] ?: ServerExtensionsLoad.Loading
                ServerExtensionsBlock(id = c.id, name = c.name.ifBlank { c.url }, url = c.url, load = load)
            }
    }
}

/** Enabled first, then by name (case-insensitive), then id, so the list is stable between reads. */
fun sortServerExtensions(list: List<ServerExtension>): List<ServerExtension> =
    list.sortedWith(compareBy<ServerExtension>({ !it.enabled }, { it.name.ifBlank { it.id }.lowercase() }, { it.id }))

fun serverExtErrorOf(e: Throwable): ServerExtError {
    val x = e as? ExtensionServerException ?: return ServerExtError.UNREACHABLE
    return when {
        x.unauthorized || x.code == ExtensionServerException.AUTHENTICATION_REQUIRED || x.code == ExtensionServerException.PAIRING_REQUIRED ->
            ServerExtError.UNAUTHORIZED
        x.code == ExtensionServerException.UNREACHABLE -> ServerExtError.UNREACHABLE
        x.code == ExtensionServerException.INCOMPATIBLE || x.code == ExtensionServerException.BAD_RESPONSE ||
            x.code == ExtensionServerException.TOO_LARGE -> ServerExtError.INCOMPATIBLE
        else -> ServerExtError.OTHER
    }
}

/** `kind` of a server extension is a comma list (`watch,read`); the distinct, non-blank parts in order. */
fun serverExtensionKinds(kind: String): List<String> = kind.split(',').map { it.trim() }.filter { it.isNotEmpty() }.distinct()
