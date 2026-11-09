// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Character
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class CharacterUiState(
    val character: LoadState<Character> = LoadState.Loading,
    /** null = no ListsRepository (the heart is hidden). */
    val favorite: Boolean? = null,
    val savingFavorite: Boolean = false,
)

sealed interface CharacterEvent {
    data class FavoriteFailed(val cause: AppError) : CharacterEvent
}

/** Classic `CharacterPage`: `MetadataRepository.character(id)` + the favorite heart through `ListsRepository` (worker q). */
class CharacterViewModel(
    private val characterId: Int,
    private val metadata: MetadataRepository,
    private val lists: ListsRepository? = null,
) : ViewModel() {
    private val _state = MutableStateFlow(CharacterUiState())
    val state: StateFlow<CharacterUiState> = _state.asStateFlow()

    private val _events = Channel<CharacterEvent>(Channel.BUFFERED)
    val events: Flow<CharacterEvent> = _events.receiveAsFlow()

    init {
        load()
        if (lists != null) {
            viewModelScope.launch {
                lists.isCharacterFavorite(characterId).catch { }.collect { fav -> _state.update { it.copy(favorite = fav) } }
            }
        }
    }

    fun retry() {
        _state.update { it.copy(character = LoadState.Loading) }
        load()
    }

    private fun load() {
        viewModelScope.launch {
            val result = runCatchingApp { metadata.character(characterId) }
            _state.update {
                it.copy(
                    character = result.fold(
                        onSuccess = { c -> if (c == null) LoadState.Err(AppError.Http(404)) else LoadState.Ok(c) },
                        onFailure = { e -> LoadState.Err(e.toAppError()) },
                    ),
                )
            }
        }
    }

    fun toggleFavorite() {
        val repo = lists ?: return
        val c = (_state.value.character as? LoadState.Ok)?.value ?: return
        val current = _state.value.favorite ?: false
        if (_state.value.savingFavorite) return
        _state.update { it.copy(savingFavorite = true, favorite = !current) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.setCharacterFavorite(CharacterRef(c.id, c.name, c.image), !current).getOrThrow() }
            _state.update { it.copy(savingFavorite = false) }
            result.onFailure { e ->
                _state.update { it.copy(favorite = current) }
                _events.trySend(CharacterEvent.FavoriteFailed(e.toAppError()))
            }
        }
    }
}
