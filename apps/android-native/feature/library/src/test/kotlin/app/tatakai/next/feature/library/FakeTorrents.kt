package app.tatakai.next.feature.library

import app.tatakai.next.model.Media
import app.tatakai.next.model.torrent.StorageLocation
import app.tatakai.next.model.torrent.TorrentEvent
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentId
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentPlayback
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.model.torrent.TorrentStorage
import app.tatakai.next.model.torrent.TorrentTracker
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.emptyFlow

/** Exposes a settable status (badge); everything else is inert. */
internal class FakeTorrents(private val result: () -> Result<TorrentId> = { Result.success("abc") }) : TorrentRepository {
    data class Call(val source: String, val mediaTitle: String, val episode: Int?)

    val downloads = mutableListOf<Call>()

    override val events = MutableSharedFlow<TorrentEvent>()
    override val library = MutableStateFlow<List<TorrentLibraryItem>>(emptyList())
    override val status = MutableStateFlow(TorrentServiceStatus())

    override suspend fun play(source: String, media: Media, episode: Int, fileIndex: Int?): Result<TorrentPlayback> =
        Result.failure(UnsupportedOperationException())

    override suspend fun playbackEnded(torrentId: TorrentId, completed: Boolean) {}

    override suspend fun addDownload(source: String, media: Media, episode: Int?): Result<TorrentId> {
        downloads += Call(source, media.title, episode)
        return result()
    }

    override fun stats(id: TorrentId): Flow<TorrentStats> = emptyFlow()
    override suspend fun files(id: TorrentId): List<TorrentFile> = emptyList()
    override fun peers(id: TorrentId): Flow<List<TorrentPeer>> = emptyFlow()
    override suspend fun trackers(id: TorrentId): List<TorrentTracker> = emptyList()
    override suspend fun remove(id: TorrentId, deleteFiles: Boolean) {}
    override suspend fun pause(id: TorrentId) {}
    override suspend fun resume(id: TorrentId) {}
    override suspend fun setPersist(id: TorrentId, persist: Boolean) {}
    override suspend fun setSeeding(id: TorrentId, seed: Boolean) {}
    override suspend fun downloadFile(id: TorrentId, fileIndex: Int) {}
    override suspend fun seekMarks(id: TorrentId, fileIndex: Int): List<ClosedFloatingPointRange<Double>> = emptyList()
    override suspend fun exportToDownloads(id: TorrentId, fileIndex: Int): Result<String> = Result.failure(UnsupportedOperationException())
    override suspend fun storageUsage(): TorrentStorage = TorrentStorage(0, 0, StorageLocation.AppSpecific)
}
