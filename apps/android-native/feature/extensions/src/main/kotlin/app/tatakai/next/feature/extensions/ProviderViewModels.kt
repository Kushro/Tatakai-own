// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.ProviderBookmark
import app.tatakai.next.model.ProviderEntry
import app.tatakai.next.model.ProviderEntryRef
import app.tatakai.next.model.ProviderItem
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// Parity wave B (16 §3.4, worker t; checklist ext.custom-providers.home / .info / .watch / .read / .bookmark / .continue): the view
// models of the provider pages (classic CustomHomePage, CustomInfoPage, CustomWatchPage, CustomReadPage).

data class ProviderUiState(
    val source: CustomSourceRef? = null,
    val home: LoadState<List<ExtensionHomeRow>> = LoadState.Loading,
    val query: String = "",
    /** null = not searching (the home rows are shown). */
    val results: LoadState<List<ExtensionHomeItem>>? = null,
    val page: Int = 1,
    val hasMore: Boolean = false,
    val loadingMore: Boolean = false,
)

class ProviderViewModel(
    private val extensionId: String,
    private val sourceId: String,
    private val repo: ExtensionRepository,
) : ViewModel() {
    private val _state = MutableStateFlow(ProviderUiState())
    val state: StateFlow<ProviderUiState> = _state.asStateFlow()
    private var searchJob: Job? = null

    init {
        load()
    }

    fun load() {
        viewModelScope.launch {
            _state.update { it.copy(home = LoadState.Loading) }
            val source = runCatchingNonCancel { repo.customSources(null).firstOrNull { it.extensionId == extensionId && it.sourceId == sourceId } }
            val home = try {
                LoadState.Ok(repo.providerHome(extensionId, sourceId).filter { it.items.isNotEmpty() })
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toError())
            }
            _state.update { it.copy(source = source ?: it.source, home = home) }
        }
    }

    fun setQuery(q: String) {
        _state.update { it.copy(query = q) }
        searchJob?.cancel()
        if (q.isBlank()) {
            _state.update { it.copy(results = null, page = 1, hasMore = false) }
            return
        }
        searchJob = viewModelScope.launch {
            delay(SEARCH_DEBOUNCE_MS)
            search(q.trim(), 1)
        }
    }

    fun retrySearch() {
        val q = _state.value.query.trim()
        if (q.isEmpty()) return
        searchJob?.cancel()
        searchJob = viewModelScope.launch { search(q, 1) }
    }

    fun loadMore() {
        val s = _state.value
        if (!s.hasMore || s.loadingMore || s.query.isBlank()) return
        searchJob = viewModelScope.launch { search(s.query.trim(), s.page + 1) }
    }

    private suspend fun search(q: String, page: Int) {
        if (page == 1) _state.update { it.copy(results = LoadState.Loading, page = 1, hasMore = false) } else _state.update { it.copy(loadingMore = true) }
        try {
            val found = repo.providerSearch(extensionId, sourceId, q, page)
            _state.update { st ->
                val prev = (st.results as? LoadState.Ok)?.value.orEmpty()
                val merged = if (page == 1) found else (prev + found).distinctBy { it.id }
                st.copy(results = LoadState.Ok(merged), page = page, hasMore = found.isNotEmpty() && merged.size > prev.size, loadingMore = false)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { st -> if (page == 1) st.copy(results = LoadState.Err(e.toError()), loadingMore = false) else st.copy(loadingMore = false, hasMore = false) }
        }
    }

    companion object {
        const val SEARCH_DEBOUNCE_MS = 400L
    }
}

enum class EntrySort { ASC, DESC }

data class ProviderItemUiState(
    val item: LoadState<ProviderItem> = LoadState.Loading,
    val kind: String = "watch",
    val bookmark: ProviderBookmark? = null,
    val filter: String = "",
    val sort: EntrySort = EntrySort.ASC,
    /** Episode number -> history row of this title (provider history id, or the AniList match). */
    val progress: Map<Int, HistoryEntry> = emptyMap(),
) {
    /** Entries after the text filter and the sort. */
    val visibleEntries: List<ProviderEntry>
        get() = filterAndSort((item as? LoadState.Ok)?.value?.entries.orEmpty(), filter, sort)
}

class ProviderItemViewModel(
    private val extensionId: String,
    private val sourceId: String,
    private val itemId: String,
    private val repo: ExtensionRepository,
    private val history: HistoryRepository? = null,
    private val clock: () -> Long = System::currentTimeMillis,
) : ViewModel() {
    private val local = MutableStateFlow(ProviderItemUiState())
    private val historyId = "ext:$extensionId:$sourceId:$itemId"

    private val bookmarkFlow: Flow<ProviderBookmark?> = repo.providerBookmarks()
        .map { list -> list.firstOrNull { it.extensionId == extensionId && it.sourceId == sourceId && it.itemId == itemId } }

    private val matchFlow: Flow<Int?> = local.map { (it.item as? LoadState.Ok)?.value?.match?.anilistId }.distinctUntilChanged()

    private val progressFlow: Flow<Map<Int, HistoryEntry>> = history?.history(HISTORY_SCAN)?.let { rows ->
        combine(rows, matchFlow) { list, anilist -> progressOf(list, historyId, anilist) }
    } ?: flowOf(emptyMap())

    val state: StateFlow<ProviderItemUiState> = combine(local, bookmarkFlow, progressFlow) { l, b, p -> l.copy(bookmark = b, progress = p) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), ProviderItemUiState())

    init {
        load()
    }

    fun load() {
        viewModelScope.launch {
            local.update { it.copy(item = LoadState.Loading) }
            val kind = runCatchingNonCancel {
                repo.customSources(null).firstOrNull { it.extensionId == extensionId && it.sourceId == sourceId }?.kind
            }
            val next: LoadState<ProviderItem> = try {
                val info = repo.providerInfo(extensionId, sourceId, itemId)
                if (info == null) LoadState.Err(app.tatakai.next.model.AppError.Unknown(NOT_FOUND)) else LoadState.Ok(info)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toError())
            }
            val entryKind = (next as? LoadState.Ok)?.value?.entries?.firstOrNull()?.kind
            local.update { it.copy(item = next, kind = kind ?: entryKind ?: it.kind) }
        }
    }

    fun setFilter(text: String) = local.update { it.copy(filter = text) }

    fun toggleSort() = local.update { it.copy(sort = if (it.sort == EntrySort.ASC) EntrySort.DESC else EntrySort.ASC) }

    /** "Guardar" / "Quitar de guardados" (ext.custom-providers.bookmark). */
    fun toggleBookmark() {
        val item = (local.value.item as? LoadState.Ok)?.value ?: return
        val current = state.value.bookmark
        viewModelScope.launch {
            runCatchingNonCancel {
                if (current != null) {
                    repo.removeProviderBookmark(extensionId, sourceId, itemId)
                } else {
                    repo.setProviderBookmark(ProviderBookmark(extensionId, sourceId, itemId, item.title, item.poster, completed = false, addedAtMs = clock()))
                }
            }
        }
    }

    /** "Completado": marks the bookmark done (saving it first when needed). */
    fun toggleCompleted() {
        val item = (local.value.item as? LoadState.Ok)?.value ?: return
        val current = state.value.bookmark
        viewModelScope.launch {
            runCatchingNonCancel {
                val base = current ?: ProviderBookmark(extensionId, sourceId, itemId, item.title, item.poster, completed = false, addedAtMs = clock())
                repo.setProviderBookmark(base.copy(completed = !base.completed))
            }
        }
    }

    /** Player arguments of a `watch` entry: the player resolves `providerWatch` and records history under the provider id. */
    fun playerArgs(entry: ProviderEntry): PlayerArgs? {
        val item = (local.value.item as? LoadState.Ok)?.value ?: return null
        return providerPlayerArgs(extensionId, sourceId, item, entry, state.value.progress)
    }

    /** Reader arguments of a `read` / `novel` entry (manga reader of worker s, with the extension's coordinates). */
    fun readerArgs(entry: ProviderEntry): MangaReaderArgs? {
        val item = (local.value.item as? LoadState.Ok)?.value ?: return null
        return MangaReaderArgs(
            mediaId = historyId,
            title = item.title,
            poster = item.poster,
            extensionId = extensionId,
            sourceId = sourceId,
            itemId = itemId,
            chapterId = entry.id,
            chapterNumber = entry.number,
            anilistId = item.match?.anilistId,
        )
    }

    /** "Continuar": the entry after the last one watched to the end (or the one in progress); null = start from the first. */
    fun continueEntry(): ProviderEntry? {
        val item = (local.value.item as? LoadState.Ok)?.value ?: return null
        return continueTarget(item.entries, state.value.progress)
    }

    companion object {
        const val HISTORY_SCAN = 500
        const val NOT_FOUND = "provider_item_not_found"
    }
}

// ---- pure helpers (unit-tested) ---------------------------------------------------------------------------------------------------

/** Episode number used by the player / history: the entry's number (rounded down, >= 1) or its 1-based position. */
fun entryEpisode(entries: List<ProviderEntry>, entry: ProviderEntry): Int {
    val n = entry.number?.takeIf { it >= 1.0 }?.toInt()
    if (n != null) return n
    val i = entries.indexOfFirst { it.id == entry.id }
    return if (i >= 0) i + 1 else 1
}

fun providerPlayerArgs(
    extensionId: String,
    sourceId: String,
    item: ProviderItem,
    entry: ProviderEntry,
    progress: Map<Int, HistoryEntry> = emptyMap(),
): PlayerArgs {
    val episode = entryEpisode(item.entries, entry)
    val resume = progress[episode]?.takeIf { !it.completed }?.progressSec?.let { it * 1000L } ?: 0L
    return PlayerArgs(
        media = item.match ?: MediaRef(),
        title = item.title,
        episode = episode,
        startMs = resume,
        provider = ProviderEntryRef(extensionId, sourceId, item.id, entry.id),
    )
}

fun filterAndSort(entries: List<ProviderEntry>, filter: String, sort: EntrySort): List<ProviderEntry> {
    val f = filter.trim()
    val filtered = if (f.isEmpty()) {
        entries
    } else {
        entries.filter { e ->
            (e.title?.contains(f, ignoreCase = true) == true) || e.number?.let { formatNumber(it) == f || formatNumber(it).startsWith(f) } == true
        }
    }
    val indexed = filtered.withIndex().sortedWith(compareBy<IndexedValue<ProviderEntry>> { it.value.number ?: Double.MAX_VALUE }.thenBy { it.index })
    val asc = indexed.map { it.value }
    return if (sort == EntrySort.ASC) asc else asc.reversed()
}

/** The entry to continue with: the first not-completed entry after the latest watched one. */
fun continueTarget(entries: List<ProviderEntry>, progress: Map<Int, HistoryEntry>): ProviderEntry? {
    if (entries.isEmpty() || progress.isEmpty()) return null
    val latest = progress.values.maxByOrNull { it.watchedAt } ?: return null
    val ordered = filterAndSort(entries, "", EntrySort.ASC)
    val idx = ordered.indexOfFirst { entryEpisode(entries, it) == latest.episode }
    if (idx < 0) return null
    return if (latest.completed) ordered.getOrNull(idx + 1) else ordered[idx]
}

/** Latest history row per episode of a provider title (its `ext:` history id, or the AniList match when the player used it). */
fun progressOf(rows: List<HistoryEntry>, historyId: String, anilistId: Int?): Map<Int, HistoryEntry> =
    rows.filter { it.animeId == historyId || (anilistId != null && it.anilistId == anilistId) }
        .groupBy { it.episode }
        .mapValues { (_, v) -> v.maxBy { it.watchedAt } }

/** `12` for 12.0, `12.5` for 12.5. */
fun formatNumber(n: Double): String = if (n % 1.0 == 0.0) n.toLong().toString() else n.toString()

private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? = try {
    block()
} catch (e: CancellationException) {
    throw e
} catch (_: Exception) {
    null
}
