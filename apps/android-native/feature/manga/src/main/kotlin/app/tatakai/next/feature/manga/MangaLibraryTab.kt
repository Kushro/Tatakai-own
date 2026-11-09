// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.MenuBook
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.MangaCatalog
import app.tatakai.next.manga.MangaRepositoryImpl
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadingProgress
import app.tatakai.next.ui.labels.readLabelRes
import app.tatakai.next.ui.media.MediaCover
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel

class MangaLibraryViewModel(
    repo: MangaRepository,
    private val catalog: MangaCatalog,
    lists: ListsRepository?,
) : ViewModel() {
    val entries: StateFlow<List<ReadEntry>?> = repo.readlist(null).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val lastRead: StateFlow<List<ReadingProgress>> = repo.lastRead(10).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
    val anilistLinked: StateFlow<Boolean> = (lists?.accounts?.map { a -> a.any { it.provider == ListProvider.ANILIST } } ?: flowOf(false))
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)
    private val _syncing = MutableStateFlow(false)
    val syncing: StateFlow<Boolean> = _syncing.asStateFlow()

    /** Pulls the AniList manga list (and retries pending pushes) when an account is linked. */
    fun sync() {
        if (_syncing.value) return
        _syncing.value = true
        viewModelScope.launch {
            runCatching { catalog.pullReadlist() }
            _syncing.value = false
        }
    }
}

/**
 * Library › Manga (`LibraryTab.MANGA`; checklist manga.readlist): status chips (labels `readLabelRes()`), rows with "x / y capítulos",
 * "Seguir leyendo"; empty state → [onOpenAccounts] when nothing local and no AniList account. Local first; AniList progress when linked.
 */
@Composable
fun MangaLibraryTab(onOpenDetails: (anilistId: Int) -> Unit, onOpenAccounts: () -> Unit, modifier: Modifier = Modifier) {
    val vm: MangaLibraryViewModel = koinViewModel()
    val entries by vm.entries.collectAsStateWithLifecycle()
    val lastRead by vm.lastRead.collectAsStateWithLifecycle()
    val linked by vm.anilistLinked.collectAsStateWithLifecycle()
    val syncing by vm.syncing.collectAsStateWithLifecycle()
    LaunchedEffect(linked) { if (linked) vm.sync() }
    ReadlistContent(
        entries = entries,
        continueItems = lastRead.mapNotNull { p ->
            MangaRepositoryImpl.refOf(p.mediaId)?.anilistId?.let { id ->
                ContinueItem(id, p.title, p.poster, stringResource(R.string.manga_continue_line, ChapterText.formatNumber(p.chapterNumber) ?: "?", p.page + 1, p.totalPages.coerceAtLeast(1)))
            }
        },
        linked = linked,
        syncing = syncing,
        onSync = vm::sync,
        onOpenDetails = onOpenDetails,
        onOpenAccounts = onOpenAccounts,
        emptyTitle = stringResource(R.string.manga_library_empty),
        emptyMessage = stringResource(if (linked) R.string.manga_library_empty_linked else R.string.manga_library_empty_message),
        modifier = modifier,
    )
}

internal data class ContinueItem(val anilistId: Int, val title: String, val poster: String?, val line: String)

@Composable
internal fun ReadlistContent(
    entries: List<ReadEntry>?,
    continueItems: List<ContinueItem>,
    linked: Boolean,
    syncing: Boolean,
    onSync: () -> Unit,
    onOpenDetails: (Int) -> Unit,
    onOpenAccounts: () -> Unit,
    emptyTitle: String,
    emptyMessage: String,
    modifier: Modifier = Modifier,
) {
    var filter by androidx.compose.runtime.saveable.rememberSaveable { androidx.compose.runtime.mutableStateOf<ListStatus?>(ListStatus.WATCHING) }
    Column(modifier) {
        if (syncing) LinearProgressIndicator(Modifier.fillMaxWidth())
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(selected = filter == null, onClick = { filter = null }, label = { Text(stringResource(R.string.manga_library_all)) })
                READ_STATUSES.forEach { s ->
                    val count = entries?.count { it.status == s } ?: 0
                    FilterChip(selected = filter == s, onClick = { filter = s }, label = { Text("${stringResource(s.readLabelRes())} · $count") })
                }
            }
            if (linked) IconButton(onClick = onSync, enabled = !syncing) { Icon(Icons.Outlined.Sync, contentDescription = stringResource(R.string.manga_library_sync)) }
        }
        val list = entries
        when {
            list == null -> Unit
            list.isEmpty() && continueItems.isEmpty() -> EmptyState(
                icon = Icons.Outlined.MenuBook,
                title = emptyTitle,
                message = emptyMessage,
                actionLabel = if (linked) null else stringResource(R.string.manga_library_link),
                onAction = if (linked) null else onOpenAccounts,
                modifier = Modifier.fillMaxSize(),
            )
            else -> {
                val shown = if (filter == null) list else list.filter { it.status == filter }
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    if (continueItems.isNotEmpty()) {
                        item(key = "continue") {
                            SectionHeader(stringResource(R.string.manga_continue_reading))
                            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                items(continueItems, key = { it.anilistId }) { c ->
                                    ContinueCard(title = c.title, cover = c.poster, line = c.line, progress = null, onClick = { onOpenDetails(c.anilistId) })
                                }
                            }
                        }
                    }
                    if (shown.isEmpty()) {
                        item(key = "none") {
                            Text(stringResource(R.string.manga_library_none_in_status), modifier = Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                    }
                    items(shown, key = { "${it.ref.anilistId}-${it.ref.malId}" }) { e ->
                        ListItem(
                            leadingContent = { Box(Modifier.width(48.dp)) { MediaCover(url = e.poster, modifier = Modifier.fillMaxWidth()) } },
                            headlineContent = { Text(e.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                            supportingContent = {
                                Column {
                                    Text(stringResource(e.status.readLabelRes()) + " · " + stringResource(R.string.manga_progress_label, e.progress, e.totalChapters?.toString() ?: "?"))
                                    e.totalChapters?.takeIf { it > 0 }?.let { total ->
                                        LinearProgressIndicator(progress = { (e.progress.toFloat() / total).coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(top = 4.dp))
                                    }
                                }
                            },
                            modifier = Modifier.clickable(enabled = e.ref.anilistId != null) { e.ref.anilistId?.let(onOpenDetails) },
                        )
                    }
                }
            }
        }
    }
}
