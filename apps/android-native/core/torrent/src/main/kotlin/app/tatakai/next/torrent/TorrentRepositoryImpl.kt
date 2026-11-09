// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import android.content.Context
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Container
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.Media
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.torrent.AfterWatch
import app.tatakai.next.model.torrent.EpisodeAlignment
import app.tatakai.next.model.torrent.TorrentSessionEntry
import app.tatakai.next.model.torrent.ProtocolStatus
import app.tatakai.next.model.torrent.StorageLocation
import app.tatakai.next.model.torrent.TORRENT_METADATA_TIMEOUT_SECS
import app.tatakai.next.model.torrent.TorrentEvent
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentId
import app.tatakai.next.model.torrent.TorrentLabel
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentPlayback
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.model.torrent.TorrentSettings
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.model.torrent.TorrentStorage
import app.tatakai.next.model.torrent.TorrentTracker
import app.tatakai.next.release.CandidateFile
import app.tatakai.next.release.isVideoPath
import app.tatakai.next.release.parseRelease
import app.tatakai.next.release.pickEpisodeFile
import app.tatakai.next.torrent.engine.EngineFile
import app.tatakai.next.torrent.engine.LibtorrentSession
import app.tatakai.next.torrent.engine.engineFiles
import app.tatakai.next.torrent.engine.episodeOf
import app.tatakai.next.torrent.engine.fileOnDisk
import app.tatakai.next.torrent.engine.isPausedFlag
import app.tatakai.next.torrent.engine.isSelected
import app.tatakai.next.torrent.engine.toModel
import app.tatakai.next.torrent.engine.toModelState
import app.tatakai.next.torrent.engine.wantedProgress
import app.tatakai.next.torrent.debrid.DebridRepositoryImpl
import app.tatakai.next.torrent.policy.AfterWatchAction
import app.tatakai.next.torrent.policy.AlignFile
import app.tatakai.next.torrent.policy.CacheItem
import app.tatakai.next.torrent.policy.PRELOAD_AT
import app.tatakai.next.torrent.policy.alignedEpisodes
import app.tatakai.next.torrent.policy.alignedIndex
import app.tatakai.next.torrent.policy.applySchedule
import app.tatakai.next.torrent.policy.cacheEvictions
import app.tatakai.next.torrent.policy.preloadTarget
import app.tatakai.next.torrent.policy.DeviceConditions
import app.tatakai.next.torrent.policy.RunFacts
import app.tatakai.next.torrent.policy.afterWatchAction
import app.tatakai.next.torrent.policy.backgroundAllowed
import app.tatakai.next.torrent.policy.deadlineWindow
import app.tatakai.next.torrent.policy.downloadedRanges
import app.tatakai.next.torrent.policy.shouldRun
import app.tatakai.next.torrent.policy.uploadLimitBytes
import app.tatakai.next.torrent.store.LabelStore
import app.tatakai.next.torrent.store.SessionHistoryStore
import app.tatakai.next.torrent.store.StoredTorrent
import app.tatakai.next.torrent.stream.LoopbackStreamServer
import app.tatakai.next.torrent.stream.StreamHandle
import app.tatakai.next.torrent.stream.StreamInfo
import app.tatakai.next.torrent.stream.StreamProvider
import app.tatakai.next.torrent.system.BatteryMonitor
import app.tatakai.next.torrent.system.NetworkMonitor
import app.tatakai.next.torrent.system.ServiceSnapshot
import app.tatakai.next.torrent.system.StorageOps
import app.tatakai.next.torrent.system.TorrentNotifications
import java.io.File
import java.io.IOException
import java.io.RandomAccessFile
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicInteger
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.BufferOverflow
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.onCompletion
import kotlinx.coroutines.flow.onStart
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import android.net.Uri
import org.libtorrent4j.AddTorrentParams
import org.libtorrent4j.AnnounceEntry
import org.libtorrent4j.Priority
import org.libtorrent4j.TorrentFlags
import org.libtorrent4j.TorrentHandle
import org.libtorrent4j.TorrentInfo
import org.libtorrent4j.alerts.Alert
import org.libtorrent4j.alerts.AlertType
import org.libtorrent4j.alerts.FileCompletedAlert
import org.libtorrent4j.alerts.TorrentAlert

/**
 * [TorrentRepository] on libtorrent4j 2.1.0-39 (docs/redesign/11, 12 §3.8 / §4 f): on-demand session, `labels.json` + resume data
 * in `filesDir/torrents`, downloads in `getExternalFilesDir("torrents")`, loopback HTTP streaming with sequential download and
 * piece deadlines, after-watch policy, no seeding after completion unless `seed`, network / battery / storage policies and the
 * foreground [TorrentService]. Single instance (Koin `torrentModule`).
 *
 * Error contract: no metadata within [TORRENT_METADATA_TIMEOUT_SECS] -> `AppError.Offline`; no matching file ->
 * `AppError.Unknown("torrent:no_file:<id>")`; plus `torrent:invalid_source`, `torrent:no_space:<bytes>`, `torrent:background_off`,
 * `torrent:missing:<id>` (not in the session any more), `torrent:moved:<id>` (moved to the SAF folder), `torrent:incomplete`,
 * `torrent:export_unsupported`.
 */
internal class TorrentRepositoryImpl(
    context: Context,
    private val settingsRepository: SettingsRepository,
    /** parity wave A (worker q, sources.debrid.*): a torrent cached on a linked debrid service plays as HTTPS, without libtorrent. */
    private val debrid: DebridRepositoryImpl? = null,
) : TorrentRepository, StreamProvider {

    private val app = context.applicationContext
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val stateDir = File(app.filesDir, "torrents")
    private val downloadDir: File
        get() = (app.getExternalFilesDir("torrents") ?: File(app.filesDir, "torrent-downloads")).also { it.mkdirs() }

    private val labels = LabelStore(File(stateDir, "labels.json"))
    private val history = SessionHistoryStore(File(stateDir, "history.json"))
    @Volatile private var scheduleActive = false
    private val session = LibtorrentSession(stateDir, ::onAlert)
    private val server = LoopbackStreamServer(this)
    private val network = NetworkMonitor(app)
    private val battery = BatteryMonitor(app)
    private val storage = StorageOps(app)
    private val engineMutex = Mutex()
    private val tickLock = Any()
    private val pieceSignal = Object()

    private val runtimes = ConcurrentHashMap<String, Runtime>()
    private val finishing = ConcurrentHashMap.newKeySet<String>()
    private val uiCollectors = AtomicInteger()
    @Volatile private var pausedBySystem = false
    @Volatile private var idleSinceMs = 0L
    @Volatile private var tickerJob: Job? = null
    @Volatile private var tickCount = 0L

    /** Set by [TorrentService] (onCreate / onDestroy). */
    @Volatile internal var serviceRunning = false

    private val _events = MutableSharedFlow<TorrentEvent>(extraBufferCapacity = 64, onBufferOverflow = BufferOverflow.DROP_OLDEST)
    override val events: SharedFlow<TorrentEvent> = _events.asSharedFlow()

    private val _status = MutableStateFlow(TorrentServiceStatus())
    override val status: StateFlow<TorrentServiceStatus> = _status.asStateFlow()

    private val _snapshot = MutableStateFlow(ServiceSnapshot())

    /** What the foreground service renders (internal to :core:torrent). */
    internal val serviceSnapshot: StateFlow<ServiceSnapshot> = _snapshot.asStateFlow()

    private val live = MutableStateFlow<Map<String, LiveInfo>>(emptyMap())

    override val library: StateFlow<List<TorrentLibraryItem>> =
        combine(labels.torrents, live) { stored, liveInfo -> buildLibrary(stored, liveInfo) }
            .stateIn(scope, SharingStarted.Eagerly, buildLibrary(labels.torrents.value, emptyMap()))

    /** The sanitized settings with the bandwidth schedule applied for the current minute (torrent.bandwidth-schedule). */
    private val settings: TorrentSettings get() = applySchedule(settingsRepository.settings.value.torrent.sanitized(), minuteOfDay())

    private fun minuteOfDay(): Int = java.time.LocalTime.now().let { it.hour * 60 + it.minute }

    init {
        network.register()
        battery.register()
        refreshIdleStatus()
        scope.launch {
            settingsRepository.settings.map { it.torrent.sanitized() }.distinctUntilChanged().collect { onSettingsChanged() }
        }
        scope.launch {
            combine(network.state, battery.low) { net, low -> net to low }.distinctUntilChanged().collect { onConditionsChanged() }
        }
        scope.launch { labels.torrents.collect { if (!session.isRunning) refreshIdleStatus() } }
        scope.launch {
            // desktop: with background downloads on, the engine starts with the app to keep saved downloads going
            delay(STARTUP_DELAY_MS)
            if (hasBackgroundWork() && backgroundAllowedNow()) {
                runCatching { ensureStarted() }
                tickSafely()
            }
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // TorrentRepository
    // ------------------------------------------------------------------------------------------------------------------------

    override suspend fun play(source: String, media: Media, episode: Int, fileIndex: Int?): Result<TorrentPlayback> =
        withContext(Dispatchers.IO) {
            var playingId: String? = null
            try {
                val (id, magnet) = resolveSource(source)
                playingId = id
                val existing = labels[id]
                if (existing?.movedUri != null) throw AppException(AppError.Unknown("torrent:moved:$id"))
                // parity wave A (sources.debrid.*): nothing local yet and a linked debrid service has it -> direct HTTPS stream
                val local = (if (session.isRunning) session.find(id) else null) != null || (existing?.progress ?: 0.0) > 0.0
                if (fileIndex == null && !local) {
                    debridPlayback(id, magnet, media, episode)?.let { return@withContext Result.success(it) }
                }
                val s = settings
                val rt = runtime(id)
                rt.endJob?.cancel()
                rt.playing = true
                val label = labelFor(media, episode, existing?.label)
                labels.upsert(
                    (existing ?: StoredTorrent(id = id, magnet = magnet, addedUnix = nowUnix())).copy(
                        label = label,
                        persist = (existing?.persist ?: false) || s.afterWatch == AfterWatch.KEEP,
                        userPaused = false,
                    ),
                )
                val handle = addOrGet(id, magnet)
                startServiceIfNeeded()
                val info = awaitMetadata(handle)
                if (info == null) {
                    rt.playing = false
                    discardIfTransient(id)
                    throw AppException(AppError.Offline)
                }
                onMetadataKnown(id, handle, info)
                val files = info.engineFiles()
                val index = fileIndex?.takeIf { i -> files.any { it.index == i } }
                    ?: alignedFor(labels[id]?.alignment, files, episode)
                    ?: pickFile(files, episode, titlesOf(media))
                    ?: throw AppException(AppError.Unknown("torrent:no_file:$id"))
                val file = files.first { it.index == index }
                val have = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY).getOrElse(index) { 0L }
                val needed = file.size - have
                if (needed > 0 && storage.allocatableBytes(downloadDir) < needed) {
                    throw AppException(AppError.Unknown("torrent:no_space:$needed"))
                }
                selectFile(id, handle, index, sequential = true)
                prefetchEdges(handle, file)
                tickSafely()
                val tags = runCatching { parseRelease(file.name) }.getOrNull()
                val stream = StreamSource(
                    url = server.url(id, index, file.name),
                    headers = emptyMap(),
                    container = Container.PROGRESSIVE,
                    quality = tags?.resolution?.label()?.takeIf { it.isNotEmpty() },
                    provider = tags?.group ?: PROVIDER_NAME,
                    extensionId = PROVIDER_ID,
                )
                val batch = files.count { isVideoPath(it.path) } > 1
                recordHistory(id, info.name(), labels[id]?.label, episode, origin = labels[id]?.origin ?: "play")
                Result.success(
                    TorrentPlayback(
                        args = PlayerArgs(media = media.ref, title = media.title, episode = episode, torrentId = id, fileIndex = index),
                        stream = stream,
                        torrentId = id,
                        fileIndex = index,
                        fileName = file.name,
                        batch = batch,
                    ),
                )
            } catch (e: CancellationException) {
                playingId?.let { runtimes[it]?.playing = false }
                throw e
            } catch (e: Exception) {
                playingId?.let { id -> runtimes[id]?.let { if (it.openStreams() == 0) it.playing = false } }
                tickSafely()
                Result.failure(e.asAppException())
            }
        }

    override suspend fun playbackEnded(torrentId: TorrentId, completed: Boolean) {
        val rt = runtimes[torrentId] ?: return
        rt.endJob?.cancel()
        rt.endJob = scope.launch {
            delay(BATCH_GRACE_MS)
            rt.playing = false
            val stored = labels[torrentId] ?: return@launch
            val handle = if (session.isRunning) session.find(torrentId) else null
            try {
                handle?.unsetFlags(TorrentFlags.SEQUENTIAL_DOWNLOAD)
                handle?.clearPieceDeadlines()
            } catch (_: Exception) {
                // handle went away
            }
            val s = settings
            val complete = stored.completed || runCatching { handle?.status(true)?.isFinished == true }.getOrDefault(false)
            val action = afterWatchAction(
                persist = stored.persist,
                backgroundDownload = stored.background,
                backgroundEnabled = s.backgroundDownloads && s.backgroundAck,
                complete = complete,
                seed = stored.seed,
                openStreams = rt.openStreams(),
                watchedToEnd = completed,
            )
            if (action == AfterWatchAction.REMOVE_WITH_FILES) {
                remove(torrentId, deleteFiles = true)
            } else {
                handle?.let(session::requestResume)
                tickSafely()
            }
        }
    }

    override suspend fun addDownload(source: String, media: Media, episode: Int?): Result<TorrentId> = withContext(Dispatchers.IO) {
        try {
            val s = settings
            if (!(s.backgroundDownloads && s.backgroundAck)) throw AppException(AppError.Unknown("torrent:background_off"))
            val (id, magnet) = resolveSource(source)
            val existing = labels[id]
            if (existing?.movedUri != null) throw AppException(AppError.Unknown("torrent:moved:$id"))
            labels.upsert(
                (existing ?: StoredTorrent(id = id, magnet = magnet, addedUnix = nowUnix())).copy(
                    label = labelFor(media, episode, existing?.label),
                    persist = true,
                    background = true,
                    wholeTorrent = episode == null || existing?.wholeTorrent == true,
                    pendingEpisode = episode,
                    userPaused = false,
                    completed = false,
                ),
            )
            val handle = addOrGet(id, magnet)
            recordHistory(id, labels[id]?.name.orEmpty(), labels[id]?.label, episode, origin = "download")
            val titles = titlesOf(media)
            scope.launch { finishBackgroundAdd(id, handle, titles, episode) }
            startServiceIfNeeded()
            tickSafely()
            Result.success(id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e.asAppException())
        }
    }

    override fun stats(id: TorrentId): Flow<TorrentStats> = flow {
        ensureStarted()
        while (currentCoroutineContextActive()) {
            val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
            emit(buildStats(id, handle))
            delay(TICK_MS)
        }
    }.trackCollector().flowOn(Dispatchers.IO)

    override suspend fun files(id: TorrentId): List<TorrentFile> = withContext(Dispatchers.IO) {
        ensureStarted()
        val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
        val info = handle.torrentFile() ?: return@withContext emptyList()
        val progress = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY)
        val priorities = handle.filePriorities()
        val rt = runtimes[id]
        val all = info.engineFiles()
        val aligned = labels[id]?.alignment?.let { a -> alignedEpisodes(alignFiles(all), a) }
        all.map { f ->
            val done = progress.getOrElse(f.index) { 0L }
            TorrentFile(
                index = f.index,
                path = f.path,
                name = f.name,
                sizeBytes = f.size,
                downloadedBytes = done,
                progress = if (f.size <= 0) 1.0 else (done.toDouble() / f.size).coerceIn(0.0, 1.0),
                selected = priorities.getOrNull(f.index)?.isSelected() == true,
                streams = rt?.streams?.get(f.index)?.get() ?: 0,
                episode = aligned?.get(f.index) ?: episodeOf(f.name),
            )
        }
    }

    override fun peers(id: TorrentId): Flow<List<TorrentPeer>> = flow {
        ensureStarted()
        while (currentCoroutineContextActive()) {
            val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
            emit(handle.peerInfo().map { it.toModel() }.sortedByDescending { it.downBps })
            delay(TICK_MS)
        }
    }.trackCollector().flowOn(Dispatchers.IO)

    override suspend fun trackers(id: TorrentId): List<TorrentTracker> = withContext(Dispatchers.IO) {
        ensureStarted()
        val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
        handle.trackers().map { it.toModel() }
    }

    override suspend fun remove(id: TorrentId, deleteFiles: Boolean): Unit = withContext(Dispatchers.IO) {
        val stored = labels[id]
        runtimes.remove(id)?.endJob?.cancel()
        val handle = if (session.isRunning) session.find(id) else null
        val finalStatus = handle?.let { h -> runCatching { h.status(true) }.getOrNull() }
        history.markRemoved(id, nowUnix(), finalStatus?.totalWantedDone(), finalStatus?.allTimeUpload())
        if (handle != null) {
            session.remove(handle, deleteFiles)
        } else if (deleteFiles && stored != null) {
            deleteLocalFiles(stored)
        }
        if (deleteFiles) stored?.movedUri?.let(storage::deleteDocument)
        session.resumeFile(id).delete()
        torrentFile(id).delete()
        labels.remove(id)
        _events.tryEmit(TorrentEvent.Removed(id))
        tickSafely()
    }

    override suspend fun pause(id: TorrentId): Unit = withContext(Dispatchers.IO) {
        labels.update(id) { it.copy(userPaused = true) }
        runtimes[id]?.userResumed = false
        session.find(id)?.let { handle ->
            runCatching { handle.pause() }
            session.requestResume(handle)
        }
        tickSafely()
    }

    override suspend fun resume(id: TorrentId): Unit = withContext(Dispatchers.IO) {
        labels.update(id) { it.copy(userPaused = false) }
        pausedBySystem = false
        runtime(id).userResumed = true
        runCatching { ensureStarted() }
        tickSafely()
        startServiceIfNeeded()
    }

    override suspend fun setPersist(id: TorrentId, persist: Boolean): Unit = withContext(Dispatchers.IO) {
        labels.update(id) { it.copy(persist = persist) }
        tickSafely()
    }

    override suspend fun setSeeding(id: TorrentId, seed: Boolean): Unit = withContext(Dispatchers.IO) {
        labels.update(id) { it.copy(seed = seed) }
        tickSafely()
    }

    override suspend fun downloadFile(id: TorrentId, fileIndex: Int): Unit = withContext(Dispatchers.IO) {
        try {
            ensureStarted()
            val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
            val info = handle.torrentFile() ?: return@withContext
            val file = info.engineFiles().firstOrNull { it.index == fileIndex } ?: return@withContext
            val have = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY).getOrElse(fileIndex) { 0L }
            val needed = file.size - have
            if (needed > 0 && storage.allocatableBytes(downloadDir) < needed) {
                throw AppException(AppError.Unknown("torrent:no_space:$needed"))
            }
            selectFile(id, handle, fileIndex, sequential = false)
            labels.update(id) { it.copy(userPaused = false, completed = false) }
            runtime(id).userResumed = true
            tickSafely()
            startServiceIfNeeded()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _events.tryEmit(TorrentEvent.Error(id, e.asAppException().error.toString()))
        }
    }

    override suspend fun seekMarks(id: TorrentId, fileIndex: Int): List<ClosedFloatingPointRange<Double>> = withContext(Dispatchers.IO) {
        if (!session.isRunning) return@withContext emptyList()
        val handle = session.find(id) ?: return@withContext emptyList()
        val file = handle.torrentFile()?.engineFiles()?.firstOrNull { it.index == fileIndex } ?: return@withContext emptyList()
        try {
            downloadedRanges(file.span) { handle.havePiece(it) }
        } catch (_: Exception) {
            emptyList()
        }
    }

    override suspend fun exportToDownloads(id: TorrentId, fileIndex: Int): Result<String> = withContext(Dispatchers.IO) {
        try {
            val stored = labels[id]
            if (stored?.movedUri != null) throw AppException(AppError.Unknown("torrent:moved:$id"))
            ensureStarted()
            val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
            val file = handle.torrentFile()?.engineFiles()?.firstOrNull { it.index == fileIndex }
                ?: throw AppException(AppError.Unknown("torrent:no_file:$id"))
            val done = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY).getOrElse(fileIndex) { 0L }
            if (done < file.size) throw AppException(AppError.Unknown("torrent:incomplete"))
            val source = handle.fileOnDisk(file)
            val uri = try {
                storage.exportToDownloads(source)
            } catch (e: IOException) {
                if (e.message == "unsupported") throw AppException(AppError.Unknown("torrent:export_unsupported"), e) else throw e
            }
            Result.success(uri)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e.asAppException())
        }
    }

    override suspend fun storageUsage(): TorrentStorage = withContext(Dispatchers.IO) {
        TorrentStorage(
            usedBytes = storage.usedBytes(downloadDir),
            freeBytes = storage.allocatableBytes(downloadDir),
            location = settings.storageLocation,
        )
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // parity wave A (15 §3.9, worker q): manual add, alignment, session history, re-announce
    // ------------------------------------------------------------------------------------------------------------------------

    override suspend fun addManual(source: String, media: Media?, alignment: EpisodeAlignment?, persist: Boolean): Result<TorrentId> =
        withContext(Dispatchers.IO) {
            try {
                val raw = source.trim()
                val fromFile = raw.startsWith("content://") || raw.startsWith("file://")
                val id: String
                val magnet: String
                var fileParams: AddTorrentParams? = null
                if (fromFile) {
                    val bytes = readTorrentBytes(raw)
                    val ti = try {
                        TorrentInfo(bytes)
                    } catch (e: Exception) {
                        throw AppException(AppError.Unknown("torrent:invalid_file"), e)
                    }
                    id = ti.infoHash().toHex().lowercase(Locale.ROOT)
                    magnet = magnetFor(id)
                    stateDir.mkdirs()
                    torrentFile(id).writeBytes(bytes)
                    fileParams = AddTorrentParams().also { it.setTorrentInfo(ti) }
                } else {
                    val resolved = resolveSource(raw)
                    id = resolved.first
                    magnet = resolved.second
                }
                val existing = labels[id]
                if (existing?.movedUri != null) throw AppException(AppError.Unknown("torrent:moved:$id"))
                val origin = if (raw.lowercase(Locale.ROOT).startsWith("magnet:")) "magnet" else "manual"
                labels.upsert(
                    (existing ?: StoredTorrent(id = id, magnet = magnet, addedUnix = nowUnix())).copy(
                        label = media?.let { labelFor(it, null, existing?.label) } ?: existing?.label,
                        persist = persist || existing?.persist == true,
                        background = persist || existing?.background == true,
                        wholeTorrent = persist || existing?.wholeTorrent == true,
                        alignment = alignment ?: existing?.alignment,
                        origin = existing?.origin?.takeIf { existing.addedUnix > 0 && it != "play" } ?: origin,
                        userPaused = false,
                        completed = false,
                    ),
                )
                val handle = fileParams?.let { addWithParams(id, it) } ?: addOrGet(id, magnet)
                // a .torrent carries its metadata: no METADATA_RECEIVED alert will come
                handle.torrentFile()?.let { onMetadataKnown(id, handle, it) }
                recordHistory(id, labels[id]?.name.orEmpty(), labels[id]?.label, null, origin)
                startServiceIfNeeded()
                tickSafely()
                Result.success(id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e.asAppException())
            }
        }

    override suspend fun setAlignment(id: TorrentId, media: Media?, alignment: EpisodeAlignment?): Result<Unit> = withContext(Dispatchers.IO) {
        val updated = labels.update(id) { t ->
            t.copy(alignment = alignment, label = media?.let { m -> labelFor(m, t.label?.episode, t.label) } ?: t.label)
        }
        if (updated == null) Result.failure(AppException(AppError.Unknown("torrent:missing:$id"))) else Result.success(Unit)
    }

    override fun sessionHistory(limit: Int): Flow<List<TorrentSessionEntry>> = history.entries.map { it.take(limit.coerceAtLeast(0)) }

    override suspend fun clearSessionHistory() = withContext(Dispatchers.IO) { history.clear() }

    override suspend fun reannounce(id: TorrentId): Unit = withContext(Dispatchers.IO) {
        ensureStarted()
        val handle = session.find(id) ?: throw AppException(AppError.Unknown("torrent:missing:$id"))
        runCatching { addCustomTrackers(handle, settings.trackers) }
        runCatching { handle.forceReannounce() }
        runCatching { handle.forceDHTAnnounce() }
        Unit
    }

    private suspend fun debridPlayback(id: String, magnet: String, media: Media, episode: Int): TorrentPlayback? {
        val d = debrid ?: return null
        if (d.accounts.value.isEmpty()) return null
        val stream = try {
            d.instantStream(id, magnet, episode)
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            null
        } ?: return null
        recordHistory(id, media.title, labelFor(media, episode, labels[id]?.label), episode, origin = "play")
        return TorrentPlayback(
            args = PlayerArgs(media = media.ref, title = media.title, episode = episode),
            stream = stream,
            torrentId = id,
            fileIndex = -1,
            fileName = stream.url.substringBefore('?').substringAfterLast('/'),
            batch = false,
        )
    }

    private fun recordHistory(id: String, name: String, label: TorrentLabel?, episode: Int?, origin: String) {
        history.record(
            TorrentSessionEntry(
                id = id,
                name = name.ifBlank { label?.title ?: id },
                mediaId = label?.mediaId?.takeIf { it.isNotBlank() },
                series = label?.title,
                episode = episode ?: label?.episode,
                addedUnix = nowUnix(),
                origin = origin,
            ),
        )
    }

    private fun alignFiles(files: List<EngineFile>): List<AlignFile> =
        files.filter { isVideoPath(it.path) }.map { AlignFile(it.index, it.path, episodeOf(it.name)) }

    private fun alignedFor(alignment: EpisodeAlignment?, files: List<EngineFile>, episode: Int): Int? =
        alignment?.let { alignedIndex(alignFiles(files), it, episode) }

    private fun addCustomTrackers(handle: TorrentHandle, trackers: List<String>) {
        if (trackers.isEmpty()) return
        val have = handle.trackers().map { it.url() }.toSet()
        trackers.filter { it !in have }.forEach { handle.addTracker(AnnounceEntry(it)) }
    }

    /** torrent.preload-next: once the streamed file is 80 % downloaded, queue the next episode's file at low priority. */
    private fun maybePreload(t: StoredTorrent, handle: TorrentHandle, info: TorrentInfo, rt: Runtime) {
        val videos = info.engineFiles().filter { isVideoPath(it.path) }
        if (videos.size < 2) return
        val aligned = t.alignment?.let { a -> alignedEpisodes(alignFiles(videos), a) }
        val list = videos.map { AlignFile(it.index, it.path, aligned?.get(it.index) ?: episodeOf(it.name)) }
        val progress = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY)
        for ((index, count) in rt.streams) {
            if (count.get() <= 0) continue
            val file = videos.firstOrNull { it.index == index } ?: continue
            val fraction = if (file.size <= 0) 1.0 else progress.getOrElse(index) { 0L }.toDouble() / file.size
            if (fraction < PRELOAD_AT) continue
            val next = preloadTarget(list, index) ?: continue
            if (!rt.preloaded.add(next)) continue
            if (!handle.filePriority(next).isSelected()) handle.filePriority(next, Priority.LOW)
            labels.update(t.id) { it.copy(selected = (it.selected + next).distinct()) }
        }
    }

    /** torrent.cache-limit: streamed (not kept) torrents beyond `cacheLimitMib` are removed with their files, oldest first. */
    private fun enforceCacheLimit(s: TorrentSettings, liveMap: Map<String, LiveInfo>) {
        val items = labels.torrents.value.values
            .filter { !it.persist && !it.background && !it.seed && it.movedUri == null }
            .map { t ->
                val size = liveMap[t.id]?.sizeBytes ?: t.sizeBytes
                val progress = liveMap[t.id]?.progress ?: t.progress
                val rt = runtimes[t.id]
                CacheItem(t.id, t.addedUnix, (size * progress).toLong(), inUse = (rt?.openStreams() ?: 0) > 0 || rt?.playing == true)
            }
        cacheEvictions(items, s.cacheLimitMib.toLong() * 1024 * 1024).forEach { id -> scope.launch { remove(id, deleteFiles = true) } }
    }

    private suspend fun addWithParams(id: String, params: AddTorrentParams): TorrentHandle {
        ensureStarted()
        session.find(id)?.let { return it }
        prepareParams(params, settings, paused = false)
        val handle = session.add(params)
        _events.tryEmit(TorrentEvent.Added(id))
        return handle
    }

    private fun torrentFile(id: String): File = File(stateDir, "$id.torrent")

    private fun torrentFileParams(id: String): AddTorrentParams? {
        val file = torrentFile(id).takeIf { it.isFile } ?: return null
        return runCatching { AddTorrentParams().also { it.setTorrentInfo(TorrentInfo(file.readBytes())) } }.getOrNull()
    }

    private fun readTorrentBytes(uri: String): ByteArray {
        val input = app.contentResolver.openInputStream(Uri.parse(uri)) ?: throw AppException(AppError.Unknown("torrent:invalid_file"))
        return input.use { stream ->
            val out = java.io.ByteArrayOutputStream()
            val buf = ByteArray(16 * 1024)
            var total = 0L
            while (true) {
                val n = stream.read(buf)
                if (n < 0) break
                total += n
                if (total > MAX_TORRENT_FILE_BYTES) throw AppException(AppError.Unknown("torrent:invalid_file"))
                out.write(buf, 0, n)
            }
            out.toByteArray()
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // Service hooks (TorrentService)
    // ------------------------------------------------------------------------------------------------------------------------

    /** Notification "Pausar todo". */
    internal fun pauseAll() {
        scope.launch {
            labels.torrents.value.values.filter { !it.completed }.forEach { t ->
                labels.update(t.id) { it.copy(userPaused = true) }
                runtimes[t.id]?.userResumed = false
            }
            tickSafely()
        }
    }

    /** Android 15 `onTimeout` of the dataSync service: pause background work until the user resumes. */
    internal fun pauseBySystem() {
        pausedBySystem = true
        tickSafely()
        refreshIdleStatus()
    }

    /** "Reanudar" of the paused-by-system notification. */
    internal fun resumeAfterSystemPause() {
        pausedBySystem = false
        scope.launch {
            runCatching { ensureStarted() }
            tickSafely()
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // StreamProvider (loopback server)
    // ------------------------------------------------------------------------------------------------------------------------

    override fun info(infoHash: String, fileIndex: Int): StreamInfo? {
        val handle = session.find(infoHash) ?: return null
        val file = handle.torrentFile()?.engineFiles()?.firstOrNull { it.index == fileIndex } ?: return null
        return StreamInfo(file.name, file.size)
    }

    override fun open(infoHash: String, fileIndex: Int): StreamHandle? {
        val handle = session.find(infoHash) ?: return null
        val file = handle.torrentFile()?.engineFiles()?.firstOrNull { it.index == fileIndex } ?: return null
        try {
            if (!handle.filePriority(fileIndex).isSelected()) selectFile(infoHash, handle, fileIndex, sequential = true)
            handle.setFlags(TorrentFlags.SEQUENTIAL_DOWNLOAD)
            if (handle.status(true).isPausedFlag()) handle.resume()
        } catch (_: Exception) {
            return null
        }
        val rt = runtime(infoHash)
        rt.streams.getOrPut(fileIndex) { AtomicInteger() }.incrementAndGet()
        _events.tryEmit(TorrentEvent.StreamOpened(infoHash, fileIndex))
        tickSafely()
        return TorrentStream(infoHash, handle, file, rt)
    }

    private inner class TorrentStream(
        private val id: String,
        private val handle: TorrentHandle,
        private val file: EngineFile,
        private val rt: Runtime,
    ) : StreamHandle {
        override val info = StreamInfo(file.name, file.size)
        private val window = deadlineWindow(file.span.pieceLength)
        private val deadlinePieces = HashSet<Int>()
        private var lastPiece = -1
        private var raf: RandomAccessFile? = null
        @Volatile private var closed = false

        override fun read(position: Long, buffer: ByteArray, offset: Int, length: Int): Int {
            if (closed) throw IOException("closed")
            if (position >= file.size) return -1
            val piece = file.span.pieceAt(position)
            awaitPiece(piece)
            val available = minOf(length.toLong(), file.span.pieceEndInFile(piece) - position, file.size - position).toInt()
            if (available <= 0) return -1
            val f = raf ?: RandomAccessFile(handle.fileOnDisk(file), "r").also { raf = it }
            f.seek(position)
            val read = f.read(buffer, offset, available)
            return if (read <= 0) throw IOException("short read") else read
        }

        private fun awaitPiece(piece: Int) {
            prioritize(piece)
            val deadline = System.currentTimeMillis() + READ_TIMEOUT_MS
            while (true) {
                if (closed) throw IOException("closed")
                val have = try {
                    handle.isValid && handle.havePiece(piece)
                } catch (_: Exception) {
                    throw IOException("torrent removed")
                }
                if (have) return
                if (!handle.isValid) throw IOException("torrent removed")
                if (System.currentTimeMillis() > deadline) throw IOException("timeout waiting for piece $piece")
                synchronized(pieceSignal) { pieceSignal.wait(PIECE_POLL_MS) }
            }
        }

        /** Deadlines on the [window] pieces from the read position; a seek drops the deadlines this stream set before. */
        private fun prioritize(piece: Int) {
            if (piece == lastPiece) return
            val jumped = lastPiece < 0 || piece < lastPiece || piece > lastPiece + window / 2
            if (!jumped) {
                lastPiece = piece
                return
            }
            try {
                val last = file.span.lastPiece
                val wanted = (piece..minOf(piece + window - 1, last)).toSet()
                deadlinePieces.filter { it !in wanted }.forEach { handle.resetPieceDeadline(it) }
                deadlinePieces.retainAll(wanted)
                wanted.sorted().forEachIndexed { i, p ->
                    if (!handle.havePiece(p)) {
                        handle.setPieceDeadline(p, DEADLINE_BASE_MS + i * DEADLINE_STEP_MS)
                        deadlinePieces += p
                    }
                }
            } catch (_: Exception) {
                // the handle may be gone; the read loop reports it
            }
            lastPiece = piece
        }

        override fun close() {
            if (closed) return
            closed = true
            runCatching { raf?.close() }
            synchronized(pieceSignal) { pieceSignal.notifyAll() }
            rt.streams[file.index]?.decrementAndGet()
            _events.tryEmit(TorrentEvent.StreamClosed(id, file.index))
            tickSafely()
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // Engine lifecycle
    // ------------------------------------------------------------------------------------------------------------------------

    private suspend fun ensureStarted() {
        if (session.isRunning && server.isRunning) return
        engineMutex.withLock {
            if (session.isRunning && server.isRunning) return
            val s = settings
            val net = network.state.value
            if (!session.isRunning) {
                session.start(s, uploadLimitBytes(s, net.metered), lsdAllowed = !net.metered)
                restoreAll(s)
            }
            if (!server.isRunning) server.start()
            idleSinceMs = 0
            startTicker()
        }
    }

    private fun restoreAll(s: TorrentSettings) {
        for (t in labels.torrents.value.values) {
            if (t.movedUri != null || session.find(t.id) != null) continue
            try {
                val params = session.readResume(t.id) ?: torrentFileParams(t.id) ?: AddTorrentParams.parseMagnetUri(t.magnet ?: magnetFor(t.id))
                prepareParams(params, s, paused = true)
                val handle = session.add(params)
                handle.torrentFile()?.let { applySelection(handle, it, t) }
            } catch (e: Exception) {
                _events.tryEmit(TorrentEvent.Error(t.id, e.message ?: "restore failed"))
            }
        }
    }

    private suspend fun shutdownEngine() {
        engineMutex.withLock {
            if (!session.isRunning) return
            tickerJob?.cancel()
            tickerJob = null
            persistProgress()
            val handles = labels.torrents.value.keys.mapNotNull { session.find(it) }
            session.stop(handles)
            server.stop()
            live.value = emptyMap()
            idleSinceMs = 0
        }
        refreshIdleStatus()
    }

    private fun startTicker() {
        if (tickerJob?.isActive == true) return
        tickerJob = scope.launch {
            while (isActive) {
                tickSafely()
                delay(TICK_MS)
            }
        }
    }

    private fun onSettingsChanged() {
        if (session.isRunning) {
            val s = settings
            val net = network.state.value
            runCatching { session.applySettings(s, uploadLimitBytes(s, net.metered), lsdAllowed = !net.metered) }
            labels.torrents.value.keys.forEach { id ->
                session.find(id)?.let { h ->
                    runCatching { h.swig().set_max_connections(s.maxPeers) }
                    runCatching { addCustomTrackers(h, s.trackers) }
                }
            }
            tickSafely()
        } else {
            refreshIdleStatus()
            maybeStartForBackground()
        }
    }

    private fun onConditionsChanged() {
        if (session.isRunning) {
            onSettingsChanged()
        } else {
            refreshIdleStatus()
            maybeStartForBackground()
        }
    }

    private fun maybeStartForBackground() {
        if (hasBackgroundWork() && backgroundAllowedNow()) {
            scope.launch {
                runCatching { ensureStarted() }
                tickSafely()
            }
        }
    }

    private fun hasBackgroundWork(): Boolean {
        val s = settings
        if (!(s.backgroundDownloads && s.backgroundAck)) return false
        return labels.torrents.value.values.any { it.movedUri == null && !it.completed && !it.userPaused && (it.background || it.persist) }
    }

    private fun backgroundAllowedNow(): Boolean = backgroundAllowed(settings, conditions())

    private fun conditions(): DeviceConditions {
        val net = network.state.value
        return DeviceConditions(connected = net.connected, metered = net.metered, batteryLow = battery.low.value, pausedBySystem = pausedBySystem)
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // Tick: run policy, live info, status, service demand, idle shutdown
    // ------------------------------------------------------------------------------------------------------------------------

    private fun tickSafely() {
        scope.launch {
            try {
                tick()
            } catch (_: Exception) {
                // a vanished handle must not stop the ticker
            }
        }
    }

    private fun tick() {
        synchronized(tickLock) { tickLocked() }
    }

    private fun tickLocked() {
        if (!session.isRunning) {
            refreshIdleStatus()
            return
        }
        tickCount++
        val s = settings
        val inWindow = s.schedule?.contains(minuteOfDay()) == true
        if (inWindow != scheduleActive) {
            // crossing a bandwidth-schedule boundary: apply the other limits to the running session
            scheduleActive = inWindow
            scope.launch { onSettingsChanged() }
        }
        val cond = conditions()
        val liveMap = HashMap<String, LiveInfo>()
        var downBps = 0L
        var upBps = 0L
        var downloads = 0
        var wanted = 0L
        var wantedDone = 0L
        var streamingTitle: String? = null
        for (t in labels.torrents.value.values) {
            if (t.movedUri != null) continue
            val handle = session.find(t.id) ?: continue
            val st = handle.status(true)
            val info = handle.torrentFile()
            val complete = st.hasMetadata() && st.totalWanted() > 0 && st.isFinished
            val progress = st.wantedProgress()
            liveMap[t.id] = LiveInfo(
                state = st.toModelState(),
                progress = if (complete) 1.0 else progress,
                sizeBytes = info?.totalSize() ?: t.sizeBytes,
                files = info?.engineFiles()?.size ?: t.files,
                path = runCatching { handle.savePath() }.getOrDefault(""),
            )
            if (complete != t.completed) onCompletionChanged(t, handle, complete)
            val rt = runtimes[t.id]
            val streaming = (rt?.openStreams() ?: 0) > 0
            val playing = rt?.playing == true
            if (streaming && streamingTitle == null) streamingTitle = t.label?.title ?: info?.name()
            if (streaming && s.preloadNext && info != null && rt != null && tickCount % PRELOAD_EVERY_TICKS == 0L) {
                runCatching { maybePreload(t, handle, info, rt) }
            }
            val run = shouldRun(
                RunFacts(
                    streaming = streaming,
                    playing = playing,
                    userPaused = t.userPaused,
                    userResumed = rt?.userResumed == true,
                    complete = complete,
                    persist = t.persist,
                    backgroundDownload = t.background,
                    seed = t.seed,
                ),
                s,
                cond,
            )
            val paused = st.isPausedFlag()
            if (run && paused) {
                handle.resume()
            } else if (!run && !paused) {
                handle.pause()
                session.requestResume(handle)
            }
            if (run && !streaming && !playing && !complete) {
                downloads++
                wanted += st.totalWanted()
                wantedDone += st.totalWantedDone()
            }
            if (run) {
                downBps += st.downloadPayloadRate().toLong().coerceAtLeast(0)
                upBps += st.uploadPayloadRate().toLong().coerceAtLeast(0)
            }
            if (tickCount % RESUME_EVERY_TICKS == 0L && run && handle.needSaveResumeData()) session.requestResume(handle)
        }
        live.value = liveMap
        if (tickCount % PERSIST_EVERY_TICKS == 0L) persistProgress()
        if (s.cacheLimitMib > 0 && tickCount % PERSIST_EVERY_TICKS == 0L) enforceCacheLimit(s, liveMap)

        val anyStreaming = runtimes.values.any { it.openStreams() > 0 }
        val anyPlaying = runtimes.values.any { it.playing }
        publishStatus(s, cond, running = true, anyStreaming = anyStreaming)
        val demand = anyStreaming || anyPlaying || downloads > 0
        _snapshot.value = ServiceSnapshot(
            demand = demand,
            streaming = anyStreaming,
            streamingTitle = streamingTitle,
            downloads = downloads,
            downBps = downBps,
            upBps = upBps,
            progress = if (wanted > 0) wantedDone.toDouble() / wanted else null,
            pausedBySystem = pausedBySystem,
        )
        if (demand) startServiceIfNeeded()

        // engine idle shutdown: nothing streams, plays, downloads and no screen collects stats for 30 s
        val now = System.currentTimeMillis()
        if (!demand && uiCollectors.get() == 0) {
            if (idleSinceMs == 0L) {
                idleSinceMs = now
            } else if (now - idleSinceMs >= IDLE_SHUTDOWN_MS) {
                idleSinceMs = 0
                scope.launch { shutdownEngine() }
            }
        } else {
            idleSinceMs = 0
        }
    }

    private fun onCompletionChanged(t: StoredTorrent, handle: TorrentHandle, complete: Boolean) {
        labels.update(t.id) { it.copy(completed = complete, progress = if (complete) 1.0 else it.progress) }
        if (!complete) return
        _events.tryEmit(TorrentEvent.Completed(t.id))
        session.requestResume(handle)
        if (t.background) {
            TorrentNotifications.notifyCompleted(app, t.id, t.label?.title ?: t.name.ifBlank { t.id }, t.label?.episode)
        }
        scope.launch { maybeMoveToUserFolder(t.id) }
    }

    private fun publishStatus(s: TorrentSettings, cond: DeviceConditions, running: Boolean, anyStreaming: Boolean) {
        val eligible = labels.torrents.value.values.filter {
            it.movedUri == null && !it.completed && !it.userPaused && (it.background || it.persist) && s.backgroundDownloads && s.backgroundAck
        }
        val streamingIds = runtimes.filterValues { it.openStreams() > 0 }.keys
        _status.value = TorrentServiceStatus(
            running = running,
            activeDownloads = eligible.count { it.id !in streamingIds },
            streaming = anyStreaming,
            metered = cond.metered,
            waitingForWifi = eligible.isNotEmpty() && s.wifiOnly && (cond.metered || !cond.connected),
            pausedLowBattery = eligible.isNotEmpty() && s.pauseOnLowBattery && cond.batteryLow,
            pausedBySystem = pausedBySystem,
        )
    }

    private fun refreshIdleStatus() {
        if (session.isRunning) return
        publishStatus(settings, conditions(), running = false, anyStreaming = false)
        _snapshot.value = ServiceSnapshot(pausedBySystem = pausedBySystem)
    }

    private fun persistProgress() {
        val current = live.value
        current.forEach { (id, info) ->
            labels.update(id) { t ->
                if (kotlin.math.abs(t.progress - info.progress) < 0.005 && t.sizeBytes == info.sizeBytes && t.files == info.files) {
                    t
                } else {
                    t.copy(progress = info.progress, sizeBytes = info.sizeBytes, files = info.files)
                }
            }
        }
    }

    private fun startServiceIfNeeded() {
        if (!serviceRunning) TorrentService.start(app)
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // Alerts
    // ------------------------------------------------------------------------------------------------------------------------

    private fun onAlert(alert: Alert<*>) {
        when (alert.type()) {
            AlertType.PIECE_FINISHED -> synchronized(pieceSignal) { pieceSignal.notifyAll() }
            AlertType.METADATA_RECEIVED -> {
                val handle = (alert as TorrentAlert<*>).handle()
                val id = handle.infoHash().toHex()
                scope.launch {
                    handle.torrentFile()?.let { onMetadataKnown(id, handle, it) }
                    _events.tryEmit(TorrentEvent.MetadataReady(id))
                }
            }
            AlertType.FILE_COMPLETED -> {
                val a = alert as FileCompletedAlert
                _events.tryEmit(TorrentEvent.FileCompleted(a.handle().infoHash().toHex(), a.index()))
                synchronized(pieceSignal) { pieceSignal.notifyAll() }
            }
            AlertType.TORRENT_FINISHED -> tickSafely()
            AlertType.TORRENT_ERROR, AlertType.FILE_ERROR -> {
                val handle = (alert as TorrentAlert<*>).handle()
                if (handle.isValid) _events.tryEmit(TorrentEvent.Error(handle.infoHash().toHex(), alert.message()))
            }
            AlertType.STATE_CHANGED -> {
                val handle = (alert as TorrentAlert<*>).handle()
                if (handle.isValid) {
                    val state = runCatching { handle.status(true).toModelState() }.getOrDefault(TorrentState.INITIALIZING)
                    _events.tryEmit(TorrentEvent.StateChanged(handle.infoHash().toHex(), state))
                }
            }
            else -> Unit
        }
    }

    private fun onMetadataKnown(id: String, handle: TorrentHandle, info: TorrentInfo) {
        val files = info.engineFiles()
        val stored = labels.update(id) {
            it.copy(name = info.name(), files = files.size, sizeBytes = info.totalSize())
        } ?: return
        applySelection(handle, info, stored)
        session.requestResume(handle)
        val pending = stored.pendingEpisode
        if (pending != null && id !in finishing) {
            val titles = listOfNotNull(stored.label?.title).filter { it.isNotBlank() }
            scope.launch { finishBackgroundAdd(id, handle, titles, pending) }
        }
    }

    // ------------------------------------------------------------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------------------------------------------------------------

    private suspend fun addOrGet(id: String, magnet: String): TorrentHandle {
        ensureStarted()
        session.find(id)?.let { return it }
        val params = session.readResume(id) ?: AddTorrentParams.parseMagnetUri(magnet)
        prepareParams(params, settings, paused = false)
        val handle = session.add(params)
        _events.tryEmit(TorrentEvent.Added(id))
        return handle
    }

    private fun prepareParams(params: AddTorrentParams, s: TorrentSettings, paused: Boolean) {
        if (params.getSavePath().isNullOrBlank()) params.setSavePath(downloadDir.absolutePath)
        params.setFlags(LibtorrentSession.addFlags(params.getFlags(), paused = paused, pex = s.pex, dontDownloadByDefault = true))
        params.setMaxConnections(s.maxPeers)
        // parity wave A (torrent.custom-trackers): the user's trackers are announced on every new torrent
        if (s.trackers.isNotEmpty()) {
            val current = runCatching { params.getTrackers() }.getOrNull().orEmpty()
            params.setTrackers((current + s.trackers).distinct())
        }
    }

    /** Re-applies the stored file selection (all files for whole-torrent downloads). */
    private fun applySelection(handle: TorrentHandle, info: TorrentInfo, stored: StoredTorrent) {
        val count = info.files().numFiles()
        val wanted: Set<Int> = if (stored.wholeTorrent) (0 until count).toSet() else stored.selected.toSet()
        if (wanted.isEmpty()) return
        val current = handle.filePriorities()
        val priorities = Array(count) { i ->
            when {
                i in wanted -> current.getOrNull(i)?.takeIf { it.isSelected() } ?: Priority.DEFAULT
                else -> current.getOrNull(i)?.takeIf { it.isSelected() } ?: Priority.IGNORE
            }
        }
        if (!priorities.contentEquals(current)) handle.prioritizeFiles(priorities)
    }

    private fun selectFile(id: String, handle: TorrentHandle, index: Int, sequential: Boolean) {
        if (!handle.filePriority(index).isSelected()) handle.filePriority(index, Priority.DEFAULT)
        if (sequential) handle.setFlags(TorrentFlags.SEQUENTIAL_DOWNLOAD)
        labels.update(id) { it.copy(selected = (it.selected + index).distinct(), completed = false) }
        if (handle.status(true).isPausedFlag()) handle.resume()
        session.requestResume(handle)
    }

    /** First and last pieces of the file first: the container index (MKV cues, MP4 moov) is often at the end. */
    private fun prefetchEdges(handle: TorrentHandle, file: EngineFile) {
        try {
            val span = file.span
            val pieces = listOf(span.firstPiece, span.firstPiece + 1, span.lastPiece - 1, span.lastPiece)
                .filter { it in span.firstPiece..span.lastPiece }
                .distinct()
            pieces.forEachIndexed { i, p -> if (!handle.havePiece(p)) handle.setPieceDeadline(p, DEADLINE_BASE_MS + i * DEADLINE_STEP_MS) }
        } catch (_: Exception) {
            // best effort
        }
    }

    private suspend fun awaitMetadata(handle: TorrentHandle): TorrentInfo? {
        val deadline = System.currentTimeMillis() + TORRENT_METADATA_TIMEOUT_SECS * 1000
        while (System.currentTimeMillis() < deadline) {
            if (!handle.isValid) return null
            handle.torrentFile()?.let { return it }
            delay(METADATA_POLL_MS)
        }
        return null
    }

    private suspend fun finishBackgroundAdd(id: String, handle: TorrentHandle, titles: List<String>, episode: Int?) {
        if (!finishing.add(id)) return
        try {
            val info = awaitMetadata(handle)
            if (info == null) {
                _events.tryEmit(TorrentEvent.Error(id, "torrent:no_metadata"))
                return
            }
            val files = info.engineFiles()
            val selected: List<Int> = if (episode == null) {
                files.map { it.index }
            } else {
                val index = pickFile(files, episode, titles)
                if (index == null) {
                    labels.update(id) { it.copy(pendingEpisode = null) }
                    _events.tryEmit(TorrentEvent.Error(id, "torrent:no_file:$id"))
                    TorrentNotifications.notifyError(app, id, labels[id]?.label?.title ?: info.name())
                    return
                }
                listOf(index)
            }
            val progress = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY)
            val needed = files.filter { it.index in selected }.sumOf { (it.size - progress.getOrElse(it.index) { 0L }).coerceAtLeast(0) }
            if (needed > 0 && storage.allocatableBytes(downloadDir) < needed) {
                labels.update(id) { it.copy(userPaused = true, pendingEpisode = null) }
                _events.tryEmit(TorrentEvent.Error(id, "torrent:no_space:$needed"))
                TorrentNotifications.notifyError(app, id, labels[id]?.label?.title ?: info.name())
                tickSafely()
                return
            }
            val stored = labels.update(id) { it.copy(selected = (it.selected + selected).distinct(), pendingEpisode = null) } ?: return
            applySelection(handle, info, stored)
            session.requestResume(handle)
            tickSafely()
        } catch (e: Exception) {
            _events.tryEmit(TorrentEvent.Error(id, e.message ?: "download failed"))
        } finally {
            finishing.remove(id)
        }
    }

    /** SAF move-on-complete (11 §6 a): copy the completed selected files to the user's folder, then drop the local copy. */
    private suspend fun maybeMoveToUserFolder(id: String) {
        val tree = (settings.storageLocation as? StorageLocation.UserFolder)?.treeUri ?: return
        val stored = labels[id] ?: return
        if (!(stored.background || stored.persist) || stored.seed || stored.movedUri != null) return
        val rt = runtimes[id]
        if ((rt?.openStreams() ?: 0) > 0 || rt?.playing == true) return
        val handle = session.find(id) ?: return
        val info = handle.torrentFile() ?: return
        val progress = handle.fileProgress(TorrentHandle.PIECE_GRANULARITY)
        val files = info.engineFiles().filter { f -> handle.filePriority(f.index).isSelected() && progress.getOrElse(f.index) { 0L } >= f.size }
        if (files.isEmpty()) return
        try {
            var first: String? = null
            for (f in files) {
                val uri = storage.copyToTree(handle.fileOnDisk(f), tree)
                if (first == null) first = uri
            }
            session.remove(handle, deleteFiles = true)
            session.resumeFile(id).delete()
            labels.update(id) { it.copy(movedUri = first, completed = true, progress = 1.0) }
            tickSafely()
        } catch (e: Exception) {
            _events.tryEmit(TorrentEvent.Error(id, "torrent:move_failed"))
        }
    }

    private fun discardIfTransient(id: String) {
        val stored = labels[id] ?: return
        if (stored.persist || stored.background) return
        scope.launch { remove(id, deleteFiles = true) }
    }

    private fun deleteLocalFiles(stored: StoredTorrent) {
        val name = stored.name.ifBlank { return }
        val root = downloadDir.canonicalFile
        val target = File(root, name).canonicalFile
        if (target.path.startsWith(root.path + File.separator)) target.deleteRecursively()
    }

    private fun buildStats(id: String, handle: TorrentHandle): TorrentStats {
        val st = handle.status(true)
        val info = handle.torrentFile()
        val stored = labels[id]
        val s = settings
        val streaming = (runtimes[id]?.openStreams() ?: 0) > 0
        val wanted = st.totalWanted()
        val done = st.totalWantedDone()
        val down = st.downloadPayloadRate().toLong().coerceAtLeast(0)
        val metered = network.state.value.metered
        val error = st.errorCode().takeIf { it.isError }?.message
        return TorrentStats(
            id = id,
            name = info?.name() ?: st.name().ifBlank { stored?.name?.ifBlank { null } ?: stored?.label?.title ?: id },
            infoHash = id,
            state = st.toModelState(),
            error = error,
            progress = st.wantedProgress(),
            downloadedBytes = done,
            uploadedBytes = st.allTimeUpload(),
            totalBytes = wanted,
            sizeBytes = info?.totalSize() ?: stored?.sizeBytes ?: 0,
            pieces = info?.numPieces() ?: 0,
            pieceBytes = info?.pieceLength()?.toLong() ?: 0,
            piecesHave = st.numPieces(),
            downBps = down,
            upBps = st.uploadPayloadRate().toLong().coerceAtLeast(0),
            seeders = st.numSeeds(),
            leechers = (st.numPeers() - st.numSeeds()).coerceAtLeast(0),
            peers = st.numPeers(),
            etaSecs = if (down > 0 && wanted > done) (wanted - done) / down else null,
            streaming = streaming,
            // libtorrent4j's TorrentStatus durations are milliseconds (swig total_milliseconds), not seconds.
            elapsedSecs = st.activeDuration() / 1000,
            protocol = ProtocolStatus(
                dht = s.dht,
                lsd = s.lsd && !metered,
                pex = s.pex,
                nat = s.upnp,
                forwarding = s.upnp && st.hasIncoming(),
                persisting = stored?.persist == true,
                streaming = streaming,
            ),
        )
    }

    private fun buildLibrary(stored: Map<String, StoredTorrent>, liveInfo: Map<String, LiveInfo>): List<TorrentLibraryItem> =
        stored.values.sortedByDescending { it.addedUnix }.map { t ->
            val l = liveInfo[t.id]
            TorrentLibraryItem(
                id = t.id,
                name = t.name.ifBlank { t.label?.title ?: t.id },
                mediaId = t.label?.mediaId?.takeIf { it.isNotBlank() },
                series = t.label?.title,
                episode = t.label?.episode,
                poster = t.label?.poster,
                files = l?.files ?: t.files,
                sizeBytes = l?.sizeBytes ?: t.sizeBytes,
                progress = if (t.completed) 1.0 else l?.progress ?: t.progress,
                state = l?.state ?: TorrentState.PAUSED,
                addedUnix = t.addedUnix,
                persist = t.persist,
                path = t.movedUri ?: l?.path?.takeIf { it.isNotBlank() }?.let { File(it, t.name).path } ?: File(downloadDir, t.name).path,
                seed = t.seed,
            )
        }

    private fun resolveSource(source: String): Pair<String, String> {
        val s = source.trim()
        val lower = s.lowercase(Locale.ROOT)
        if (HEX40.matches(lower)) return lower to (labels[lower]?.magnet ?: magnetFor(lower))
        if (lower.startsWith("magnet:")) {
            val params = try {
                AddTorrentParams.parseMagnetUri(s)
            } catch (e: Exception) {
                throw AppException(AppError.Unknown("torrent:invalid_source"), e)
            }
            val hashes = params.getInfoHashes()
            val id = (if (hashes.hasV1()) hashes.getV1() else hashes.getBest()).toHex().lowercase(Locale.ROOT)
            return id to s
        }
        throw AppException(AppError.Unknown("torrent:invalid_source"))
    }

    private fun pickFile(files: List<EngineFile>, episode: Int, titles: List<String>): Int? = try {
        pickEpisodeFile(files.map { CandidateFile(it.index, it.path, it.size) }, episode, titles)
    } catch (_: Throwable) {
        // the release parser is worker a's port; a failure means "let the user pick in Files"
        null
    }

    private fun labelFor(media: Media, episode: Int?, previous: TorrentLabel?): TorrentLabel = TorrentLabel(
        mediaId = HistoryEntry.animeIdOf(media.ref.anilistId, media.ref.malId) ?: previous?.mediaId.orEmpty(),
        title = media.title,
        episode = episode,
        poster = media.cover ?: previous?.poster,
    )

    private fun titlesOf(media: Media): List<String> =
        (listOf(media.title) + media.titles.values).map { it.trim() }.filter { it.isNotEmpty() }.distinct()

    private fun runtime(id: String): Runtime = runtimes.getOrPut(id) { Runtime() }

    private fun <T> Flow<T>.trackCollector(): Flow<T> =
        onStart { uiCollectors.incrementAndGet() }.onCompletion { uiCollectors.decrementAndGet() }

    private suspend fun currentCoroutineContextActive(): Boolean = kotlinx.coroutines.currentCoroutineContext().isActive

    private fun Exception.asAppException(): AppException = when (this) {
        is AppException -> this
        is IOException -> AppException(AppError.Unknown(message ?: "io"), this)
        else -> AppException(AppError.Unknown(message ?: this::class.java.simpleName), this)
    }

    /** Per-torrent runtime facts (not persisted). */
    private class Runtime {
        val streams = ConcurrentHashMap<Int, AtomicInteger>()
        @Volatile var playing: Boolean = false
        @Volatile var userResumed: Boolean = false
        @Volatile var endJob: Job? = null
        /** Files already queued by the next-episode preload. */
        val preloaded: MutableSet<Int> = ConcurrentHashMap.newKeySet()

        fun openStreams(): Int = streams.values.sumOf { it.get().coerceAtLeast(0) }
    }

    /** Engine-side facts merged into [library]. */
    private data class LiveInfo(val state: TorrentState, val progress: Double, val sizeBytes: Long, val files: Int, val path: String)

    companion object {
        const val PROVIDER_ID: String = "torrent"
        private const val PROVIDER_NAME = "Torrent"
        private const val TICK_MS = 1_000L
        private const val BATCH_GRACE_MS = 20_000L
        private const val IDLE_SHUTDOWN_MS = 30_000L
        private const val STARTUP_DELAY_MS = 1_500L
        private const val METADATA_POLL_MS = 250L
        private const val READ_TIMEOUT_MS = 120_000L
        private const val PIECE_POLL_MS = 200L
        private const val DEADLINE_BASE_MS = 300
        private const val DEADLINE_STEP_MS = 150
        private const val PERSIST_EVERY_TICKS = 30L
        private const val RESUME_EVERY_TICKS = 60L
        private const val PRELOAD_EVERY_TICKS = 5L
        private const val MAX_TORRENT_FILE_BYTES = 10L * 1024 * 1024
        private val HEX40 = Regex("[0-9a-f]{40}")

        private fun magnetFor(id: String): String = "magnet:?xt=urn:btih:$id"

        private fun nowUnix(): Long = System.currentTimeMillis() / 1000
    }
}
