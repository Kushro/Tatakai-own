package app.tatakai.next.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onStart
import kotlinx.coroutines.flow.stateIn

/** Local library only (E7): what the user is watching plus the full history. AniList/MAL lists arrive in M1. */
data class LibraryData(
    val inProgress: List<HistoryEntry>,
    val history: List<HistoryEntry>,
) {
    val isEmpty: Boolean get() = history.isEmpty()
}

fun List<HistoryEntry>.toLibraryData(): LibraryData {
    val sorted = sortedByDescending { it.watchedAt }
    return LibraryData(inProgress = sorted.filter { !it.completed }, history = sorted)
}

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryViewModel(history: HistoryRepository) : ViewModel() {

    private val reload = MutableStateFlow(0)

    val state: StateFlow<LoadState<LibraryData>> = reload
        .flatMapLatest {
            history.continueWatching(HISTORY_LIMIT)
                .map<List<HistoryEntry>, LoadState<LibraryData>> { LoadState.Ok(it.toLibraryData()) }
                .onStart { emit(LoadState.Loading) }
                .catch { e ->
                    if (e is CancellationException) throw e
                    emit(LoadState.Err(e.toAppError()))
                }
        }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), LoadState.Loading)

    fun retry() {
        reload.value += 1
    }

    private companion object {
        const val HISTORY_LIMIT = 200
    }
}
