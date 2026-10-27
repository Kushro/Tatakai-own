package app.tatakai.next.feature.extensions

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.InstalledExtension
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

data class ExtensionsUiState(
    val items: List<InstalledExtension> = emptyList(),
    val installing: Boolean = false,
    val message: InstallMessage? = null,
)

class ExtensionsViewModel(private val repo: ExtensionRepository) : ViewModel() {

    private val transient = MutableStateFlow(Transient())

    val state: StateFlow<ExtensionsUiState> = combine(repo.installed, transient) { items, t ->
        ExtensionsUiState(items = items, installing = t.installing, message = t.message)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ExtensionsUiState(items = repo.installed.value))

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
        }
    }

    fun setEnabled(id: String, enabled: Boolean) {
        viewModelScope.launch { repo.setEnabled(id, enabled) }
    }

    fun remove(id: String) {
        viewModelScope.launch { repo.remove(id) }
    }

    fun dismissMessage() = transient.update { it.copy(message = null) }

    private data class Transient(val installing: Boolean = false, val message: InstallMessage? = null)
}
