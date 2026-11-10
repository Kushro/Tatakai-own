// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ReadEntry
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.serialization.Serializable
import java.io.File

@Serializable
internal data class ReadlistDoc(
    val entries: List<ReadEntry> = emptyList(),
    /** AniList ids whose last local change has not reached AniList yet (retried by [ReadlistStore.pull]). */
    val pending: Set<Int> = emptySet(),
)

/**
 * The manga / novel readlist (checklist manga.readlist / novel.readlist / manga.readlist.anilist-sync): local first, mirrored to the
 * user's AniList list with `SaveMediaListEntry` when an AniList account is linked (`ListsRepository.accessToken(ANILIST)`; the token is
 * asked for every time and never stored here). A failed push keeps the change local and marks it pending; [pull] retries pending pushes
 * and merges the AniList list (newer `updatedAt` wins). [novels] selects which AniList formats belong to this list.
 */
class ReadlistStore(
    file: File,
    private val anilist: AniListMangaApi,
    private val lists: ListsRepository?,
    private val novels: Boolean,
    private val clock: () -> Long = System::currentTimeMillis,
    /** `AppSettings.listsWriteBack`: off = local edits never reach AniList (not even later as pending); [pull] still merges. */
    private val writeBack: () -> Boolean = { true },
) {
    private val store = JsonFileStore(file, ReadlistDoc.serializer()) { ReadlistDoc() }
    private val _entries = MutableStateFlow<List<ReadEntry>>(emptyList())
    val entries: StateFlow<List<ReadEntry>> = _entries.asStateFlow()

    suspend fun load() {
        _entries.value = sorted(store.read().entries)
    }

    fun flow(status: ListStatus?): Flow<List<ReadEntry>> = entries.map { list -> if (status == null) list else list.filter { it.status == status } }

    fun entry(ref: MediaRef): Flow<ReadEntry?> = entries.map { list -> list.firstOrNull { same(it.ref, ref) } }

    fun current(ref: MediaRef): ReadEntry? = _entries.value.firstOrNull { same(it.ref, ref) }

    suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> {
        val now = clock()
        val doc = store.update { doc ->
            val rest = doc.entries.filterNot { same(it.ref, media.ref) }
            if (status == null) {
                doc.copy(entries = rest)
            } else {
                val old = doc.entries.firstOrNull { same(it.ref, media.ref) }
                val entry = ReadEntry(
                    ref = media.ref,
                    title = media.title,
                    poster = media.cover ?: old?.poster,
                    status = status,
                    progress = when {
                        status == ListStatus.COMPLETED && media.episodes != null -> maxOf(old?.progress ?: 0, media.episodes ?: 0)
                        else -> old?.progress ?: 0
                    },
                    totalChapters = media.episodes ?: old?.totalChapters,
                    score = old?.score,
                    updatedAtMs = now,
                )
                doc.copy(entries = rest + entry)
            }
        }
        _entries.value = sorted(doc.entries)
        val anilistId = media.ref.anilistId ?: return Result.success(Unit)
        return push(anilistId) { token ->
            if (status == null) {
                anilist.deleteEntry(token, anilistId)
            } else {
                anilist.saveEntry(token, anilistId, status, current(media.ref)?.progress)
            }
        }
    }

    /**
     * Chapters read; PLANNING (or no status) becomes WATCHING (= Leyendo) on the first chapter and COMPLETED when it reaches the total.
     * [title] / [poster] create the entry when the title was not on the list yet (finishing a chapter adds it, like the classic reader).
     */
    suspend fun setProgress(ref: MediaRef, chapters: Int, title: String? = null, poster: String? = null): Result<Unit> {
        val now = clock()
        val doc = store.update { doc ->
            val old = doc.entries.firstOrNull { same(it.ref, ref) }
            if (old == null && title == null) return@update doc
            val base = old ?: ReadEntry(ref = ref, title = title.orEmpty(), poster = poster, status = ListStatus.WATCHING, updatedAtMs = now)
            val total = base.totalChapters
            val status = when {
                total != null && total > 0 && chapters >= total -> ListStatus.COMPLETED
                base.status == ListStatus.PLANNING || base.status == ListStatus.PAUSED || base.status == ListStatus.DROPPED -> ListStatus.WATCHING
                else -> base.status
            }
            val updated = base.copy(progress = chapters.coerceAtLeast(0), status = status, updatedAtMs = now)
            doc.copy(entries = doc.entries.filterNot { same(it.ref, ref) } + updated)
        }
        _entries.value = sorted(doc.entries)
        val entry = current(ref) ?: return Result.failure(AppException(AppError.Unknown("not on the readlist")))
        val anilistId = ref.anilistId ?: return Result.success(Unit)
        return push(anilistId) { token -> anilist.saveEntry(token, anilistId, entry.status, entry.progress) }
    }

    /** Pushes pending changes, then merges the AniList list. Returns the number of entries added or updated locally. */
    suspend fun pull(): Result<Int> {
        val token = lists?.accessToken(ListProvider.ANILIST) ?: return Result.success(0)
        return try {
            // from the file, not [entries]: a pull right after app start may run before [load] filled them (a missing entry deletes remotely)
            if (writeBack()) {
                val saved = store.read()
                saved.pending.forEach { id ->
                    val entry = saved.entries.firstOrNull { it.ref.anilistId == id }
                    if (entry == null) anilist.deleteEntry(token, id) else anilist.saveEntry(token, id, entry.status, entry.progress)
                }
                store.update { it.copy(pending = emptySet()) }
            }
            val remote = anilist.readList(token).filter { isNovel(it.format) == novels }
            var changed = 0
            val doc = store.update { doc ->
                val merged = merge(doc.entries, remote) { changed = it }
                doc.copy(entries = merged)
            }
            _entries.value = sorted(doc.entries)
            Result.success(changed)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    suspend fun wipe() {
        store.clear()
        _entries.value = emptyList()
    }

    private suspend fun push(anilistId: Int, call: suspend (String) -> Unit): Result<Unit> {
        // "Enviar los cambios a las cuentas" off: the change stays on this phone
        if (!writeBack()) return Result.success(Unit)
        val token = runCatching { lists?.accessToken(ListProvider.ANILIST) }.getOrNull() ?: return Result.success(Unit)
        return try {
            call(token)
            store.update { it.copy(pending = it.pending - anilistId) }
            Result.success(Unit)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Local change kept; retried on the next pull.
            store.update { it.copy(pending = it.pending + anilistId) }
            Result.success(Unit)
        }
    }

    companion object {
        fun isNovel(format: String?): Boolean = format.equals("NOVEL", ignoreCase = true)

        fun same(a: MediaRef, b: MediaRef): Boolean =
            (a.anilistId != null && a.anilistId == b.anilistId) || (a.anilistId == null && a.malId != null && a.malId == b.malId)

        private fun sorted(list: List<ReadEntry>): List<ReadEntry> = list.sortedByDescending { it.updatedAtMs }

        /** Remote rows win when newer than the local row (AniList `updatedAt` is in seconds); local-only rows are kept. */
        fun merge(local: List<ReadEntry>, remote: List<AniListReadRow>, onChanged: (Int) -> Unit = {}): List<ReadEntry> {
            var changed = 0
            val byId = local.associateBy { it.ref.anilistId }.toMutableMap()
            val out = local.toMutableList()
            remote.forEach { row ->
                val id = row.media.ref.anilistId ?: return@forEach
                val remoteMs = (row.updatedAtSec ?: 0L) * 1000L
                val mine = byId[id]
                if (mine == null || remoteMs > mine.updatedAtMs) {
                    val entry = ReadEntry(
                        ref = row.media.ref,
                        title = row.media.title,
                        poster = row.media.cover ?: mine?.poster,
                        status = row.status,
                        progress = row.progress,
                        totalChapters = row.totalChapters ?: mine?.totalChapters,
                        score = row.score ?: mine?.score,
                        updatedAtMs = if (remoteMs > 0) remoteMs else (mine?.updatedAtMs ?: 0L),
                    )
                    if (mine != entry) {
                        out.removeAll { it.ref.anilistId == id }
                        out += entry
                        byId[id] = entry
                        changed++
                    }
                }
            }
            onChanged(changed)
            return out
        }
    }
}
