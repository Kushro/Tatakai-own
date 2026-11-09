// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.library

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onStart
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import java.text.Collator
import java.util.Locale

// parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3, worker q; checklist lists.library-tabs, lists.library-sort-filter,
// lists.favorites, lists.local-only, character.favorite): state of Library › Listas and Library › Favoritos.

/** Sort orders of Library › Listas (classic `WatchlistPage` sort menu). */
enum class ListSort { UPDATED, TITLE, PROGRESS, SCORE }

/** Sorted copy; ties keep the repository order (newest change first). */
internal fun List<ListEntry>.sortedFor(sort: ListSort, locale: Locale = Locale.getDefault()): List<ListEntry> = when (sort) {
    ListSort.UPDATED -> sortedByDescending { it.updatedAtMs }
    ListSort.TITLE -> {
        val collator = Collator.getInstance(locale).apply { strength = Collator.PRIMARY }
        sortedWith { a, b -> collator.compare(a.title, b.title) }
    }
    ListSort.PROGRESS -> sortedWith(compareByDescending<ListEntry> { it.progressFraction() ?: -1f }.thenByDescending { it.progress })
    ListSort.SCORE -> sortedWith(compareByDescending<ListEntry> { it.score ?: -1 }.thenByDescending { it.updatedAtMs })
}

/** Case- and accent-insensitive title filter; blank = everything. */
internal fun List<ListEntry>.filteredBy(query: String): List<ListEntry> {
    val q = query.trim().normalizedForSearch()
    if (q.isEmpty()) return this
    return filter { it.title.normalizedForSearch().contains(q) || it.notes?.normalizedForSearch()?.contains(q) == true }
}

internal fun String.normalizedForSearch(): String =
    java.text.Normalizer.normalize(lowercase(Locale.ROOT), java.text.Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")

/** progress / total, null when the total is unknown. */
internal fun ListEntry.progressFraction(): Float? = totalEpisodes?.takeIf { it > 0 }?.let { (progress.toFloat() / it).coerceIn(0f, 1f) }

/** Minimal [Media] for repository writes made from a [ListEntry] (Library cards, the editor sheet). */
internal fun ListEntry.toMedia(): Media = Media(
    ref = ref,
    kind = MediaKind.ANIME,
    title = title,
    titles = emptyMap(),
    cover = poster,
    banner = null,
    color = null,
    synopsis = null,
    genres = emptyList(),
    episodes = totalEpisodes,
    status = null,
    season = null,
    year = null,
    score = null,
    sourceProvider = "lists",
)

data class ListsUiState(
    val status: ListStatus = ListStatus.WATCHING,
    val sort: ListSort = ListSort.UPDATED,
    val query: String = "",
    /** Entries of [status], already sorted and filtered. */
    val entries: LoadState<List<ListEntry>> = LoadState.Loading,
    /** Count per status (segmented badges). */
    val counts: Map<ListStatus, Int> = emptyMap(),
    val favorites: LoadState<List<ListEntry>> = LoadState.Loading,
    val favoriteCharacters: List<CharacterRef> = emptyList(),
    val accounts: List<LinkedAccount> = emptyList(),
    val pendingChanges: Int = 0,
)

sealed interface ListsEvent {
    /** A write landed locally while offline / with pending pushes: "Se sincronizará". */
    data object Queued : ListsEvent
    data class Failed(val cause: AppError) : ListsEvent
}

@OptIn(ExperimentalCoroutinesApi::class)
class ListsViewModel(private val lists: ListsRepository) : ViewModel() {

    private data class Controls(val status: ListStatus = ListStatus.WATCHING, val sort: ListSort = ListSort.UPDATED, val query: String = "")

    private val controls = MutableStateFlow(Controls())
    private val reload = MutableStateFlow(0)
    private val _events = Channel<ListsEvent>(Channel.BUFFERED)
    val events: Flow<ListsEvent> = _events.receiveAsFlow()

    private val all: Flow<LoadState<List<ListEntry>>> = reload.flatMapLatest {
        lists.entries(null).asLoadState()
    }

    private val favorites: Flow<LoadState<List<ListEntry>>> = reload.flatMapLatest {
        lists.favorites().asLoadState()
    }

    private val characters: Flow<List<CharacterRef>> = lists.favoriteCharacters().catch { emit(emptyList()) }

    val state: StateFlow<ListsUiState> = combine(
        controls,
        all,
        favorites,
        characters,
        combine(lists.accounts, lists.pendingChanges) { a, p -> a to p },
    ) { c, everything, favs, chars, (accounts, pending) ->
        val counts = (everything as? LoadState.Ok)?.value?.groupingBy { it.status }?.eachCount().orEmpty()
        val entries = when (everything) {
            is LoadState.Ok -> LoadState.Ok(everything.value.filter { it.status == c.status }.filteredBy(c.query).sortedFor(c.sort))
            is LoadState.Err -> everything
            LoadState.Loading -> LoadState.Loading
        }
        ListsUiState(
            status = c.status,
            sort = c.sort,
            query = c.query,
            entries = entries,
            counts = counts,
            favorites = favs,
            favoriteCharacters = chars,
            accounts = accounts,
            pendingChanges = pending,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ListsUiState())

    fun setStatusFilter(status: ListStatus) = controls.value.let { controls.value = it.copy(status = status) }

    fun setSort(sort: ListSort) = controls.value.let { controls.value = it.copy(sort = sort) }

    fun setQuery(query: String) = controls.value.let { controls.value = it.copy(query = query.take(MAX_QUERY)) }

    fun retry() {
        reload.value += 1
    }

    fun setStatus(entry: ListEntry, status: ListStatus?) = write { lists.setStatus(entry.toMedia(), status) }

    fun setProgress(ref: MediaRef, episode: Int) = write { lists.setProgress(ref, episode.coerceAtLeast(0)) }

    fun setScore(ref: MediaRef, score: Int?) = write { lists.setScore(ref, score?.coerceIn(0, 100)) }

    fun setFavorite(entry: ListEntry, favorite: Boolean) = write { lists.setFavorite(entry.toMedia(), favorite) }

    fun removeCharacter(character: CharacterRef) = write { lists.setCharacterFavorite(character, false) }

    private fun write(block: suspend () -> Result<Unit>) {
        viewModelScope.launch {
            val result = try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
            result.fold(
                onSuccess = { if (lists.accounts.value.isNotEmpty() && lists.pendingChanges.value > 0) _events.trySend(ListsEvent.Queued) },
                onFailure = { _events.trySend(ListsEvent.Failed(it.toAppError())) },
            )
        }
    }

    private fun Flow<List<ListEntry>>.asLoadState(): Flow<LoadState<List<ListEntry>>> =
        map<List<ListEntry>, LoadState<List<ListEntry>>> { LoadState.Ok(it) }
            .onStart { emit(LoadState.Loading) }
            .catch { e ->
                if (e is CancellationException) throw e
                emit(LoadState.Err(e.toAppError()))
            }

    private companion object {
        const val MAX_QUERY = 100
    }
}
