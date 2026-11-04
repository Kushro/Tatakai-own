package app.tatakai.next.feature.extensions

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.ExtensionDetail
import app.tatakai.next.model.ExtensionLogEntry
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.InstalledExtension
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** One-shot outcome of the last install attempt, shown inline (never as stacked toasts). */
sealed interface InstallMessage {
    data class Installed(val name: String) : InstallMessage

    /** [reason] is the manifest/validation error reported by the host, so the user sees which field failed. */
    data class Failed(val reason: String) : InstallMessage
}

/** Hash / domains / date of one row ([ExtensionRepository.detail]). */
sealed interface DetailState {
    data object Loading : DetailState

    /** [detail] null = the repository knows nothing more about it (the foot line is hidden). */
    data class Ready(val detail: ExtensionDetail?) : DetailState

    data object Failed : DetailState
}

data class ExtensionsUiState(
    val items: List<InstalledExtension> = emptyList(),
    val installing: Boolean = false,
    val message: InstallMessage? = null,
    /** True while the installed list has not been read yet (the screen shows the loading indicator after 150 ms). */
    val loading: Boolean = false,
    val details: Map<String, DetailState> = emptyMap(),
    /** Rows whose collapsible log is open. */
    val expandedLogs: Set<String> = emptySet(),
)

class ExtensionsViewModel(private val repo: ExtensionRepository) : ViewModel() {

    private val transient = MutableStateFlow(Transient())
    private val details = MutableStateFlow<Map<String, DetailState>>(emptyMap())
    private val expanded = MutableStateFlow<Set<String>>(emptySet())

    /** The repository has no "ready" signal: an empty list counts as loading until it has had [LOAD_GRACE_MS] to fill. */
    private val settled = MutableStateFlow(repo.installed.value.isNotEmpty())

    /** id -> version whose detail was requested, so a reinstall (new hash) reloads it. */
    private val requested = HashMap<String, String>()

    val state: StateFlow<ExtensionsUiState> =
        combine(repo.installed, transient, details, expanded, settled) { items, t, d, e, s ->
            ExtensionsUiState(
                items = items,
                installing = t.installing,
                message = t.message,
                loading = items.isEmpty() && !s,
                details = d.filterKeys { id -> items.any { it.id == id } },
                expandedLogs = e,
            )
        }.stateIn(
            viewModelScope,
            SharingStarted.WhileSubscribed(5_000),
            ExtensionsUiState(items = repo.installed.value, loading = repo.installed.value.isEmpty()),
        )

    init {
        viewModelScope.launch {
            delay(LOAD_GRACE_MS)
            settled.value = true
        }
        viewModelScope.launch {
            repo.installed.collect { items ->
                if (items.isNotEmpty()) settled.value = true
                items.forEach { ext -> if (requested[ext.id] != ext.version) loadDetail(ext) }
                requested.keys.retainAll(items.map { it.id }.toSet())
            }
        }
    }

    /** [uri] comes from the SAF document picker (`.kai` package). */
    fun install(uri: String) {
        if (transient.value.installing) return
        transient.value = Transient(installing = true)
        viewModelScope.launch {
            val result = repo.installFromUri(uri)
            transient.value = Transient(
                message = result.fold(
                    onSuccess = { InstallMessage.Installed(it.name) },
                    onFailure = { InstallMessage.Failed(it.message ?: it::class.java.simpleName) },
                ),
            )
            // A reinstall of the same version still changes the package hash.
            result.getOrNull()?.let { loadDetail(it) }
        }
    }

    fun setEnabled(id: String, enabled: Boolean) {
        viewModelScope.launch { repo.setEnabled(id, enabled) }
    }

    fun remove(id: String) {
        viewModelScope.launch { repo.remove(id) }
    }

    fun dismissMessage() = transient.update { it.copy(message = null) }

    fun toggleLog(id: String) = expanded.update { if (id in it) it - id else it + id }

    fun retryDetail(id: String) {
        repo.installed.value.firstOrNull { it.id == id }?.let(::loadDetail)
    }

    fun logs(id: String): Flow<List<ExtensionLogEntry>> = repo.logs(id)

    private fun loadDetail(ext: InstalledExtension) {
        requested[ext.id] = ext.version
        details.update { it + (ext.id to DetailState.Loading) }
        viewModelScope.launch {
            val next = try {
                DetailState.Ready(repo.detail(ext.id))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                DetailState.Failed
            }
            details.update { it + (ext.id to next) }
        }
    }

    private data class Transient(val installing: Boolean = false, val message: InstallMessage? = null)

    companion object {
        const val LOAD_GRACE_MS = 600L
    }
}
