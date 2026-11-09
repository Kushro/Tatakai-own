// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.UpdateInfo
import app.tatakai.next.model.UpdateRepository
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.io.File

// parity wave A (15 §3.4, worker q; checklist system.open-logs, system.update-check, system.export-history,
// system.clear-watch-history): the Acerca de / Datos actions that are not plain settings.

/** Result of "Buscar actualizaciones": null = never asked in this screen. */
sealed interface UpdateCheckState {
    data object Idle : UpdateCheckState
    data object Checking : UpdateCheckState
    data object UpToDate : UpdateCheckState
    data class Available(val info: UpdateInfo) : UpdateCheckState
    data class Failed(val cause: AppError) : UpdateCheckState
}

data class SystemUiState(
    val update: UpdateCheckState = UpdateCheckState.Idle,
    /** Last lines of the app log; Loading until "Abrir registros" asks for them. */
    val logs: LoadState<List<String>> = LoadState.Loading,
    val exportingHistory: Boolean = false,
    val clearingHistory: Boolean = false,
)

sealed interface SystemEvent {
    /** The history export document; the screen writes it to the SAF file the user picked. */
    data class HistoryExported(val json: String) : SystemEvent
    data object HistoryCleared : SystemEvent
    data object LogsCleared : SystemEvent
    data class Failed(val cause: AppError) : SystemEvent
}

class SystemViewModel(
    private val logs: LogRepository? = null,
    private val updates: UpdateRepository? = null,
    private val history: HistoryRepository? = null,
) : ViewModel() {

    private val local = MutableStateFlow(SystemUiState())
    private val _events = Channel<SystemEvent>(Channel.BUFFERED)
    val events: Flow<SystemEvent> = _events.receiveAsFlow()

    val state: StateFlow<SystemUiState> = combine(local, updates?.latest ?: flowOf(null)) { l, latest ->
        // a newer release announced at startup shows up here too, without pressing the button
        if (latest != null && l.update == UpdateCheckState.Idle) l.copy(update = UpdateCheckState.Available(latest)) else l
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), SystemUiState())

    val hasUpdates: Boolean get() = updates != null
    val hasLogs: Boolean get() = logs != null

    fun checkUpdates() {
        val repo = updates ?: return
        if (local.value.update == UpdateCheckState.Checking) return
        local.update { it.copy(update = UpdateCheckState.Checking) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.check(force = true).getOrThrow() }
            local.update {
                it.copy(
                    update = result.fold(
                        onSuccess = { info -> if (info == null) UpdateCheckState.UpToDate else UpdateCheckState.Available(info) },
                        onFailure = { e -> UpdateCheckState.Failed(e.toAppError()) },
                    ),
                )
            }
        }
    }

    fun loadLogs() {
        val repo = logs ?: run {
            local.update { it.copy(logs = LoadState.Ok(emptyList())) }
            return
        }
        local.update { it.copy(logs = LoadState.Loading) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.recentLines(MAX_LOG_LINES) }
            local.update { s -> s.copy(logs = result.fold(onSuccess = { LoadState.Ok(it) }, onFailure = { LoadState.Err(it.toAppError()) })) }
        }
    }

    fun logFile(): File? = logs?.logFile()

    fun clearLogs() {
        val repo = logs ?: return
        viewModelScope.launch {
            runCatchingApp { repo.clear() }.fold(
                onSuccess = {
                    local.update { it.copy(logs = LoadState.Ok(emptyList())) }
                    _events.trySend(SystemEvent.LogsCleared)
                },
                onFailure = { _events.trySend(SystemEvent.Failed(it.toAppError())) },
            )
        }
    }

    fun exportHistory() {
        val repo = history ?: return
        if (local.value.exportingHistory) return
        local.update { it.copy(exportingHistory = true) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.exportJson() }
            local.update { it.copy(exportingHistory = false) }
            result.fold(
                onSuccess = { _events.trySend(SystemEvent.HistoryExported(it)) },
                onFailure = { _events.trySend(SystemEvent.Failed(it.toAppError())) },
            )
        }
    }

    /** [includeRemote] only after the "también en la PC" confirmation. */
    fun clearHistory(includeRemote: Boolean) {
        val repo = history ?: return
        if (local.value.clearingHistory) return
        local.update { it.copy(clearingHistory = true) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.clearHistory(includeRemote).getOrThrow() }
            local.update { it.copy(clearingHistory = false) }
            result.fold(
                onSuccess = { _events.trySend(SystemEvent.HistoryCleared) },
                onFailure = { _events.trySend(SystemEvent.Failed(it.toAppError())) },
            )
        }
    }

    private companion object {
        const val MAX_LOG_LINES = 500
    }
}
