// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AutoDownloadPreview
import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadProfile
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentState
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the six tabs draw, already filtered by the search field. */
data class DownloadsUiState(
    val tab: DownloadsTab = DownloadsTab.ACTIVE,
    val query: String = "",
    val active: List<DownloadItem> = emptyList(),
    val activeTorrents: List<TorrentLibraryItem> = emptyList(),
    /** Completed web episodes grouped by title (Anime tab). */
    val anime: List<TitleGroup> = emptyList(),
    val completedTorrents: List<TorrentLibraryItem> = emptyList(),
    val manga: List<ChapterDownload> = emptyList(),
    val novels: List<ChapterDownload> = emptyList(),
    val history: List<DownloadItem> = emptyList(),
    val library: List<LocalLibraryItem> = emptyList(),
    val rules: List<AutoDownloadRule> = emptyList(),
    val usedBytes: Long? = null,
    /** ETA in seconds per download id (null = not known yet). */
    val eta: Map<String, Long> = emptyMap(),
    val loaded: Boolean = false,
    val busy: Boolean = false,
)

data class TitleGroup(val ref: MediaRef, val title: String, val poster: String?, val episodes: List<DownloadItem>) {
    val bytes: Long get() = episodes.sumOf { it.totalBytes ?: it.downloadedBytes }
}

/** One-shot messages for the snackbar. */
sealed interface DownloadsEvent {
    data class Checked(val queued: Int) : DownloadsEvent
    data class Verified(val broken: Int) : DownloadsEvent
    data class Imported(val added: Int) : DownloadsEvent
    data class Preview(val preview: AutoDownloadPreview?) : DownloadsEvent
    data class Failed(val cause: Throwable) : DownloadsEvent
}

/** Pure filters / grouping of the Downloads page (JVM-tested). */
internal object DownloadsLogic {
    fun matches(query: String, vararg fields: String?): Boolean {
        val q = query.trim()
        if (q.isEmpty()) return true
        return fields.any { it?.contains(q, ignoreCase = true) == true }
    }

    fun active(items: List<DownloadItem>): List<DownloadItem> = items.filter {
        it.state == DownloadState.QUEUED || it.state == DownloadState.DOWNLOADING || it.state == DownloadState.PAUSED || it.state == DownloadState.FAILED
    }

    fun groups(items: List<DownloadItem>): List<TitleGroup> = items
        .filter { it.state == DownloadState.COMPLETED || it.state == DownloadState.BROKEN }
        .groupBy { it.ref }
        .map { (ref, eps) ->
            val first = eps.maxBy { it.completedAtMs ?: it.createdAtMs }
            TitleGroup(ref, first.title, first.poster, eps.sortedBy { it.episode })
        }
        .sortedByDescending { g -> g.episodes.maxOf { it.completedAtMs ?: it.createdAtMs } }

    fun torrentActive(t: TorrentLibraryItem): Boolean = t.progress < 1.0 && t.state != TorrentState.SEEDING

    /** Remaining seconds from two samples of the progress (0..1) of one download; null while it cannot be estimated. */
    fun eta(p0: Double, t0Ms: Long, p1: Double, t1Ms: Long): Long? {
        val dp = p1 - p0
        val dt = t1Ms - t0Ms
        if (dp <= 0.0 || dt <= 0 || p1 >= 1.0) return null
        return ((1.0 - p1) * dt / dp / 1000.0).toLong()
    }
}

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r, checklist downloads.page): one state for the six tabs
 * (`DownloadsTab`): web downloads from [downloads], torrents from [torrents] (library of :core:torrent, finished by q), chapters from
 * every [chapters] source (manga / novels of s), the local library, search, ETA, auto-download rules.
 */
class DownloadsViewModel(
    private val downloads: DownloadRepository,
    private val torrents: TorrentRepository? = null,
    private val chapters: List<ChapterDownloadSource> = emptyList(),
    private val clock: () -> Long = System::currentTimeMillis,
) : ViewModel() {
    val items: StateFlow<List<DownloadItem>> = downloads.items

    private val local = MutableStateFlow(Local())
    private val samples = HashMap<String, Pair<Double, Long>>()
    private val _events = MutableSharedFlow<DownloadsEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<DownloadsEvent> = _events

    private data class Local(val tab: DownloadsTab = DownloadsTab.ACTIVE, val query: String = "", val used: Long? = null, val busy: Boolean = false)

    private val chapterFlow: Flow<List<ChapterDownload>> =
        if (chapters.isEmpty()) {
            flowOf(emptyList())
        } else {
            combine(chapters.map { src -> src.downloads().catch { emit(emptyList()) } }) { lists -> lists.toList().flatten() }
        }

    private val sources = combine(
        downloads.items,
        downloads.history(),
        downloads.library,
        downloads.rules,
        torrents?.library?.catch { emit(emptyList()) } ?: flowOf(emptyList()),
    ) { web, hist, lib, rules, tor -> Sources(web, hist, lib, rules, tor) }

    private data class Sources(
        val web: List<DownloadItem>,
        val history: List<DownloadItem>,
        val library: List<LocalLibraryItem>,
        val rules: List<AutoDownloadRule>,
        val torrents: List<TorrentLibraryItem>,
    )

    val state: StateFlow<DownloadsUiState> = combine(local, sources, chapterFlow) { l, s, ch ->
        val q = l.query
        DownloadsUiState(
            tab = l.tab,
            query = q,
            active = DownloadsLogic.active(s.web).filter { DownloadsLogic.matches(q, it.title, it.sourceLabel) },
            activeTorrents = s.torrents.filter { DownloadsLogic.torrentActive(it) && DownloadsLogic.matches(q, it.name, it.series) },
            anime = DownloadsLogic.groups(s.web).filter { DownloadsLogic.matches(q, it.title) },
            completedTorrents = s.torrents.filter { !DownloadsLogic.torrentActive(it) && DownloadsLogic.matches(q, it.name, it.series) },
            manga = ch.filter { it.kind == ChapterKind.MANGA && DownloadsLogic.matches(q, it.title, it.chapterLabel) },
            novels = ch.filter { it.kind == ChapterKind.NOVEL && DownloadsLogic.matches(q, it.title, it.chapterLabel) },
            history = s.history.filter { DownloadsLogic.matches(q, it.title, it.sourceLabel) },
            library = s.library.filter { DownloadsLogic.matches(q, it.fileName, it.title) },
            rules = s.rules,
            usedBytes = l.used,
            eta = etaOf(s.web),
            loaded = true,
            busy = l.busy,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), DownloadsUiState())

    init {
        refreshUsage()
    }

    private fun etaOf(web: List<DownloadItem>): Map<String, Long> {
        val now = clock()
        val out = HashMap<String, Long>()
        web.filter { it.state == DownloadState.DOWNLOADING }.forEach { item ->
            val first = samples.getOrPut(item.id) { item.progress to now }
            DownloadsLogic.eta(first.first, first.second, item.progress, now)?.let { out[item.id] = it }
        }
        samples.keys.retainAll(web.filter { it.state == DownloadState.DOWNLOADING }.map { it.id }.toSet())
        return out
    }

    fun selectTab(tab: DownloadsTab) = local.update { it.copy(tab = tab) }

    fun setQuery(q: String) = local.update { it.copy(query = q) }

    fun refreshUsage() {
        viewModelScope.launch {
            val used = runCatching { downloads.storageUsedBytes() }.getOrNull()
            local.update { it.copy(used = used) }
        }
    }

    fun pause(id: String) = launch { downloads.pause(id) }
    fun resume(id: String) = launch { downloads.resume(id) }
    fun retry(id: String) = launch { downloads.retry(id) }
    fun remove(id: String) = launch {
        downloads.remove(id)
        refreshUsage()
    }
    fun clearHistory() = launch { downloads.clearHistory() }

    fun removeTitle(group: TitleGroup) = launch {
        group.episodes.forEach { downloads.remove(it.id) }
        refreshUsage()
    }

    fun pauseTorrent(id: String) = launch { torrents?.pause(id) }
    fun resumeTorrent(id: String) = launch { torrents?.resume(id) }

    fun removeChapter(c: ChapterDownload) = launch { chapters.firstOrNull { it.kind == c.kind }?.remove(c.id) }
    fun retryChapter(c: ChapterDownload) = launch { chapters.firstOrNull { it.kind == c.kind }?.retry(c.id) }

    fun verify() = busy {
        val broken = downloads.verifyIntegrity()
        _events.tryEmit(DownloadsEvent.Verified(broken.size))
    }

    fun checkNow() = busy {
        downloads.checkNow().fold(
            onSuccess = { _events.tryEmit(DownloadsEvent.Checked(it)) },
            onFailure = { _events.tryEmit(DownloadsEvent.Failed(it)) },
        )
    }

    fun setRule(rule: AutoDownloadRule) = launch { downloads.setRule(rule) }

    fun addRule(ref: MediaRef, title: String, poster: String?, profile: DownloadProfile) =
        launch { downloads.setRule(AutoDownloadRule(ref = ref, title = title, poster = poster, profile = profile)) }

    fun preview(ref: MediaRef) = launch { _events.tryEmit(DownloadsEvent.Preview(downloads.preview(ref))) }

    fun importFiles(uris: List<String>) = busy {
        downloads.importFiles(uris).fold(
            onSuccess = { _events.tryEmit(DownloadsEvent.Imported(it.size)) },
            onFailure = { _events.tryEmit(DownloadsEvent.Failed(it)) },
        )
    }

    fun scanFolder(treeUri: String) = busy {
        downloads.scanFolder(treeUri).fold(
            onSuccess = { _events.tryEmit(DownloadsEvent.Imported(it.size)) },
            onFailure = { _events.tryEmit(DownloadsEvent.Failed(it)) },
        )
    }

    fun matchLocal(id: String, media: Media?, episode: Int?) = launch { downloads.matchLocal(id, media, episode) }
    fun removeLocal(id: String, deleteFile: Boolean) = launch { downloads.removeLocal(id, deleteFile) }

    private fun launch(block: suspend () -> Unit) {
        viewModelScope.launch {
            runCatching { block() }.onFailure { if (it is kotlinx.coroutines.CancellationException) throw it else _events.tryEmit(DownloadsEvent.Failed(it)) }
        }
    }

    private fun busy(block: suspend () -> Unit) {
        local.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                runCatching { block() }.onFailure { if (it is kotlinx.coroutines.CancellationException) throw it else _events.tryEmit(DownloadsEvent.Failed(it)) }
            } finally {
                local.update { it.copy(busy = false) }
                refreshUsage()
            }
        }
    }
}
