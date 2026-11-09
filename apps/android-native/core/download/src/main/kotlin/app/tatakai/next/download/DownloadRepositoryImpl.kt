// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.offline.Download
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.offline.DownloadService
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AutoDownloadPreview
import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadProfile
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadRequest
import app.tatakai.next.model.DownloadSettings
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.DownloadsSummary
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.NotificationExtras
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import org.koin.core.context.GlobalContext
import java.io.File
import java.time.LocalDate

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r): [DownloadRepository] over the Media3
 * `DownloadManager` of [DownloadEngine] (`DefaultDownloadIndex` on a `StandaloneDatabaseProvider`, `SimpleCache` in the download folder,
 * OkHttp with the extension headers of each request). HLS = `HlsDownloader` (all segments of the chosen variant, resumes from the
 * index), MP4 = `ProgressiveDownloader` (Range). [TatakaiDownloadService] keeps the manager alive in the foreground. When Media3
 * reports a download complete, its bytes are written into ONE playable file (`<folder>/<mediaId>/<episode>.ts|mp4`, HLS segments
 * concatenated; see KNOWN-ISSUES), copied into the SAF folder when the user chose one, and the Media3 copy is dropped: `localUri` is
 * what the player and the external app open.
 *
 * Private stores (JSON under `filesDir/downloads/`, [DownloadStore]): downloads (item + stream + headers), rules, library, hidden history.
 */
@UnstableApi
class DownloadRepositoryImpl(
    private val context: Context,
    private val settings: () -> DownloadSettings,
    private val extensions: ExtensionRepository,
    private val metadata: MetadataRepository,
    private val history: HistoryRepository,
    /** Auto-download of WATCHING titles (`AppSettings.downloads.autoCheck`); null = lists not bound. */
    private val lists: ListsRepository? = null,
    private val storeDir: File = File(context.filesDir, DIR_NAME),
    private val clock: () -> Long = System::currentTimeMillis,
) : DownloadRepository, AppDataWiper {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val store = DownloadStore(storeDir)
    private val mutex = Mutex()

    private val http: OkHttpClient by lazy { runCatching { GlobalContext.getOrNull()?.getOrNull<OkHttpClient>() }.getOrNull() ?: OkHttpClient() }
    private val sniffer: EmbedSniffer? get() = runCatching { GlobalContext.getOrNull()?.getOrNull<EmbedSniffer>() }.getOrNull()

    internal val engine: DownloadEngine by lazy { DownloadEngine(context, http) }

    /** The manager the service drives (Koin `single<DownloadManager>` in `downloadModule`). */
    val manager: DownloadManager get() = engine.manager

    private val records = MutableStateFlow<Map<String, StoredDownload>>(emptyMap())
    private val hidden = MutableStateFlow<Set<String>>(emptySet())
    /** Ids whose Media3 copy is being dropped after the export (their `onDownloadRemoved` is not a user removal). */
    private val exported = java.util.Collections.synchronizedSet(HashSet<String>())
    private val exporting = java.util.Collections.synchronizedSet(HashSet<String>())

    override val items: StateFlow<List<DownloadItem>> =
        records.map { m -> DownloadLogic.sorted(m.values.map { it.item }) }.stateIn(scope, SharingStarted.Eagerly, emptyList())
    override val summary: StateFlow<DownloadsSummary> = items.map(DownloadLogic::summary).stateIn(scope, SharingStarted.Eagerly, DownloadsSummary())

    private val _rules = MutableStateFlow<List<AutoDownloadRule>>(emptyList())
    override val rules: StateFlow<List<AutoDownloadRule>> = _rules.asStateFlow()
    private val _library = MutableStateFlow<List<LocalLibraryItem>>(emptyList())
    override val library: StateFlow<List<LocalLibraryItem>> = _library.asStateFlow()

    private var applied: DownloadSettings? = null

    private val listener = object : DownloadManager.Listener {
        override fun onDownloadChanged(downloadManager: DownloadManager, download: Download, finalException: Exception?) {
            onMedia3Changed(download, finalException)
        }

        override fun onDownloadRemoved(downloadManager: DownloadManager, download: Download) {
            exported.remove(download.request.id)
        }
    }

    init {
        scope.launch {
            val stored = store.downloads()
            stored.forEach { engine.registerHeaders(it.requestUrl, it.stream.headers) }
            // REMOVING items of a previous run never came back: drop them.
            records.value = stored.filter { it.item.state != DownloadState.REMOVING }.associateBy { it.item.id }
            hidden.value = store.hiddenHistory().toSet()
            _rules.value = store.rules()
            _library.value = store.library()
            withContext(Dispatchers.Main) {
                runCatching {
                    manager.addListener(listener)
                    applySettings(settings())
                }
            }
            recoverFinished()
            progressLoop()
        }
        scope.launch { evictWatchedLoop() }
    }

    // ------------------------------------------------------------------ Media3 events

    private fun onMedia3Changed(download: Download, error: Exception?) {
        val id = download.request.id
        val rec = records.value[id] ?: return
        if (id in exported) return
        val state = when (download.state) {
            Download.STATE_QUEUED, Download.STATE_RESTARTING -> DownloadState.QUEUED
            Download.STATE_STOPPED -> DownloadState.PAUSED
            Download.STATE_DOWNLOADING -> DownloadState.DOWNLOADING
            Download.STATE_FAILED -> DownloadState.FAILED
            Download.STATE_REMOVING -> DownloadState.REMOVING
            Download.STATE_COMPLETED -> DownloadState.DOWNLOADING
            else -> rec.item.state
        }
        val progress = progressOf(download, rec.item.progress)
        update(id, persist = true) {
            it.copy(
                state = state,
                progress = if (download.state == Download.STATE_COMPLETED) 1.0 else progress,
                downloadedBytes = download.bytesDownloaded,
                totalBytes = download.contentLength.takeIf { l -> l > 0 } ?: it.totalBytes,
                error = if (state == DownloadState.FAILED) (error?.message ?: ERR_FAILED).take(200) else null,
            )
        }
        if (state == DownloadState.FAILED) DownloadNotifications.failed(context, rec.item)
        if (download.state == Download.STATE_COMPLETED) scope.launch { exportCompleted(id) }
    }

    private fun progressOf(d: Download, fallback: Double): Double {
        val pct = d.percentDownloaded
        return if (pct == C.PERCENTAGE_UNSET.toFloat() || pct < 0f) fallback else (pct / 100f).toDouble().coerceIn(0.0, 1.0)
    }

    /** Downloads Media3 finished while the process was dead (or before the listener existed): export them now. */
    private suspend fun recoverFinished() {
        val pending = records.value.values.filter { it.item.state == DownloadState.QUEUED || it.item.state == DownloadState.DOWNLOADING }
        for (rec in pending) {
            val d = runCatching { manager.downloadIndex.getDownload(rec.item.id) }.getOrNull()
            when {
                d == null -> resend(rec, foreground = false)
                d.state == Download.STATE_COMPLETED -> exportCompleted(rec.item.id)
                d.state == Download.STATE_FAILED -> update(rec.item.id, persist = true) { it.copy(state = DownloadState.FAILED, error = ERR_FAILED) }
            }
        }
    }

    /** Polls the bytes of the running downloads (Media3 only reports state changes) and re-applies changed settings. */
    private suspend fun progressLoop() {
        var tick = 0
        while (true) {
            delay(PROGRESS_TICK_MS)
            tick++
            val active = records.value.values.any { it.item.state == DownloadState.DOWNLOADING || it.item.state == DownloadState.QUEUED }
            val s = settings()
            withContext(Dispatchers.Main) {
                runCatching {
                    if (s != applied) applySettings(s)
                    if (active) {
                        manager.currentDownloads.forEach { d ->
                            val id = d.request.id
                            val rec = records.value[id] ?: return@forEach
                            if (d.state != Download.STATE_DOWNLOADING || id in exporting) return@forEach
                            val p = progressOf(d, rec.item.progress)
                            if (p != rec.item.progress || d.bytesDownloaded != rec.item.downloadedBytes) {
                                update(id, persist = tick % PERSIST_EVERY == 0) {
                                    it.copy(
                                        state = DownloadState.DOWNLOADING,
                                        progress = p,
                                        downloadedBytes = d.bytesDownloaded,
                                        totalBytes = d.contentLength.takeIf { l -> l > 0 } ?: it.totalBytes,
                                    )
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /** Settings › Descargas: concurrency and the Wi-Fi rule go straight to the manager (main thread). */
    internal fun applySettings(s: DownloadSettings) {
        val clean = s.sanitized()
        manager.maxParallelDownloads = clean.maxConcurrent
        manager.requirements = engine.requirementsFor(clean.wifiOnly)
        applied = s
    }

    private suspend fun exportCompleted(id: String) {
        if (!exporting.add(id)) return
        try {
            val rec = records.value[id] ?: return
            val item = rec.item
            val s = settings()
            val dirName = DownloadLogic.dirName(item.ref)
            val name = DownloadLogic.fileName(item.episode, item.container)
            val file = File(File(engine.rootDir, dirName), name)
            val result = runCatching {
                withContext(Dispatchers.IO) {
                    val written = engine.export(rec.requestUrl, item.container, file)
                    val tree = s.directoryTreeUri
                    val saf = tree?.let { runCatching { engine.copyToTree(it, dirName, name, DownloadLogic.mimeOf(item.container), file) }.getOrNull() }
                    if (saf != null) file.delete()
                    written to (saf ?: Uri.fromFile(file).toString())
                }
            }
            result.fold(
                onSuccess = { (written, uri) ->
                    update(id, persist = true) {
                        it.copy(
                            state = DownloadState.COMPLETED,
                            progress = 1.0,
                            downloadedBytes = written,
                            totalBytes = written,
                            localUri = uri,
                            error = null,
                            completedAtMs = clock(),
                        )
                    }
                    // Drop the Media3 copy (segments in the cache): the exported file is the download now.
                    exported.add(id)
                    sendRemove(id)
                    records.value[id]?.let { DownloadNotifications.done(context, it.item) }
                },
                onFailure = { e ->
                    if (e is CancellationException) throw e
                    update(id, persist = true) { it.copy(state = DownloadState.FAILED, error = "export: ${e.message}".take(200)) }
                    DownloadNotifications.failed(context, item)
                },
            )
        } finally {
            exporting.remove(id)
        }
    }

    // ------------------------------------------------------------------ queue

    override suspend fun enqueue(request: DownloadRequest): Result<DownloadItem> = enqueueInternal(request, foreground = true)

    private suspend fun enqueueInternal(request: DownloadRequest, foreground: Boolean): Result<DownloadItem> = runCatchingIo {
        val stream = request.stream
        val ref = request.media.ref
        if (stream.container == Container.DASH) throw AppException(AppError.Extension(stream.extensionId, ERR_DASH))
        val s = settings().sanitized()
        if (DownloadLogic.overLimit(storageUsedBytes(), s.storageLimitGib)) throw AppException(AppError.Extension(stream.extensionId, ERR_LIMIT))
        val id = DownloadLogic.itemId(ref, request.episode)
        records.value[id]?.let { existing ->
            if (existing.item.state != DownloadState.FAILED && existing.item.state != DownloadState.BROKEN) return@runCatchingIo existing.item
        }
        val requestUrl = if (stream.container == Container.HLS) {
            engine.resolveHls(stream.url, stream.headers, s.autoProfile)
        } else {
            stream.url
        }
        val item = DownloadItem(
            id = id,
            ref = ref,
            title = request.media.title,
            poster = request.media.cover,
            episode = request.episode,
            extId = stream.extensionId,
            sourceLabel = request.sourceLabel,
            container = if (stream.container == Container.HLS) Container.HLS else Container.MP4,
            state = DownloadState.QUEUED,
            createdAtMs = clock(),
        )
        val rec = StoredDownload(item = item, stream = stream, requestUrl = requestUrl, media = request.media)
        engine.registerHeaders(requestUrl, stream.headers)
        engine.registerHeaders(stream.url, stream.headers)
        mutex.withLock {
            records.update { it + (id to rec) }
            persist()
        }
        hidden.update { it - id }
        resend(rec, foreground)
        item
    }

    private suspend fun resend(rec: StoredDownload, foreground: Boolean) {
        val req = androidx.media3.exoplayer.offline.DownloadRequest.Builder(rec.item.id, Uri.parse(rec.requestUrl))
            .setMimeType(engine.mimeFor(rec.item.container))
            .setData(rec.item.title.take(120).toByteArray())
            .build()
        val sent = runCatching { DownloadService.sendAddDownload(context, TatakaiDownloadService::class.java, req, foreground) }.isSuccess
        if (!sent) withContext(Dispatchers.Main) { runCatching { manager.addDownload(req) } }
    }

    private suspend fun sendRemove(id: String) {
        val sent = runCatching { DownloadService.sendRemoveDownload(context, TatakaiDownloadService::class.java, id, false) }.isSuccess
        if (!sent) withContext(Dispatchers.Main) { runCatching { manager.removeDownload(id) } }
    }

    private suspend fun sendStopReason(id: String, reason: Int) {
        val sent = runCatching { DownloadService.sendSetStopReason(context, TatakaiDownloadService::class.java, id, reason, false) }.isSuccess
        if (!sent) withContext(Dispatchers.Main) { runCatching { manager.setStopReason(id, reason) } }
    }

    override suspend fun enqueueSeason(media: Media, episodes: List<Int>, preferredServer: String?, preferredAudioLang: String?): Result<List<DownloadItem>> =
        runCatchingIo {
            val existing = records.value.values.map { it.item }.filter { sameRef(it.ref, media.ref) }.associateBy { it.episode }
            val todo = DownloadLogic.missingEpisodes(episodes, existing)
            val queued = ArrayList<DownloadItem>()
            var lastError: Throwable? = null
            for (ep in todo) {
                val r = resolveStream(media, ep, preferredServer, preferredAudioLang, settings().autoProfile)
                if (r == null) {
                    lastError = AppException(AppError.Extension("", ERR_NO_SOURCE))
                    continue
                }
                enqueueInternal(DownloadRequest(media, ep, r.first, r.second), foreground = true)
                    .onSuccess { queued += it }
                    .onFailure { lastError = it }
            }
            if (queued.isEmpty() && todo.isNotEmpty()) throw lastError ?: AppException(AppError.Extension("", ERR_NO_SOURCE))
            queued
        }

    /** Best downloadable stream of an episode for [profile] (embed pages are sniffed first; DASH skipped). */
    private suspend fun resolveStream(media: Media, episode: Int, server: String?, lang: String?, profile: DownloadProfile): Pair<StreamSource, String>? {
        val ranked = runCatching { extensions.resolveCandidates(media, episode) }.getOrDefault(emptyList())
        for (r in DownloadLogic.candidatesFor(ranked, profile, server, lang)) {
            val c: SourceCandidate = r.candidate
            val stream = if (c.isEmbed) {
                val w = c.web ?: continue
                val sn = sniffer ?: continue
                runCatching { sn.sniff(w.url, w.headers, c.extId).toStream(c) }.getOrNull()
            } else {
                c.toStream()
            } ?: continue
            if (stream.container == Container.DASH) continue
            return stream to c.label
        }
        // No ranked candidate: the plain streams of the extensions (wave android path).
        val plain = runCatching { extensions.resolveStreams(media, episode) }.getOrDefault(emptyList())
        return plain.firstOrNull { it.container != Container.DASH }?.let { it to it.provider }
    }

    override suspend fun pause(id: String) {
        sendStopReason(id, STOP_PAUSED)
        update(id, persist = true) { it.copy(state = DownloadState.PAUSED) }
    }

    override suspend fun resume(id: String) {
        val rec = records.value[id] ?: return
        update(id, persist = true) { it.copy(state = DownloadState.QUEUED) }
        val known = withContext(Dispatchers.IO) { runCatching { manager.downloadIndex.getDownload(id) }.getOrNull() }
        if (known == null) resend(rec, foreground = true) else sendStopReason(id, Download.STOP_REASON_NONE)
    }

    override suspend fun remove(id: String) {
        val rec = records.value[id] ?: return
        update(id, persist = false) { it.copy(state = DownloadState.REMOVING) }
        sendRemove(id)
        withContext(Dispatchers.IO) {
            rec.item.localUri?.let(engine::delete)
            File(File(engine.rootDir, DownloadLogic.dirName(rec.item.ref)), DownloadLogic.fileName(rec.item.episode, rec.item.container)).delete()
        }
        mutex.withLock {
            records.update { it - id }
            persist()
        }
    }

    override suspend fun retry(id: String) {
        val rec = records.value[id] ?: return
        val media = rec.media
        rec.item.localUri?.let { withContext(Dispatchers.IO) { engine.delete(it) } }
        // URLs of extensions expire: re-resolve the same server when the title is known, else reuse the stored stream.
        val fresh = media?.let { resolveStream(it, rec.item.episode, rec.item.sourceLabel, settings().preferredAudioLang, settings().autoProfile) }
        val stream = fresh?.first ?: rec.stream
        val requestUrl = runCatchingIo {
            if (stream.container == Container.HLS) engine.resolveHls(stream.url, stream.headers, settings().autoProfile) else stream.url
        }.getOrDefault(rec.requestUrl)
        engine.registerHeaders(requestUrl, stream.headers)
        val next = rec.copy(
            stream = stream,
            requestUrl = requestUrl,
            item = rec.item.copy(state = DownloadState.QUEUED, progress = 0.0, downloadedBytes = 0, localUri = null, error = null, completedAtMs = null),
        )
        mutex.withLock {
            records.update { it + (id to next) }
            persist()
        }
        exported.remove(id)
        sendRemove(id)
        resend(next, foreground = true)
    }

    override fun history(limit: Int): Flow<List<DownloadItem>> =
        combine(items, hidden) { list, h -> DownloadLogic.history(list.filter { it.id !in h }, limit) }

    override suspend fun clearHistory() {
        val failed = records.value.values.filter { it.item.state == DownloadState.FAILED || it.item.state == DownloadState.BROKEN }.map { it.item.id }
        failed.forEach { sendRemove(it) }
        mutex.withLock {
            records.update { m -> m - failed.toSet() }
            persist()
        }
        val done = records.value.values.filter { it.item.state == DownloadState.COMPLETED }.map { it.item.id }
        hidden.value = done.toSet()
        withContext(Dispatchers.IO) { store.saveHiddenHistory(done) }
    }

    override suspend fun localUriFor(ref: MediaRef, episode: Int): String? = withContext(Dispatchers.IO) {
        val done = records.value.values.map { it.item }
            .firstOrNull { it.state == DownloadState.COMPLETED && it.episode == episode && sameRef(it.ref, ref) && it.localUri != null }
        done?.localUri?.takeIf { engine.stat(it).first }
            ?: _library.value.firstOrNull { it.ref != null && sameRef(it.ref!!, ref) && it.episode == episode }?.uri?.takeIf { engine.stat(it).first }
    }

    override fun downloadedEpisodes(ref: MediaRef): Flow<Map<Int, DownloadItem>> = items.map { list ->
        list.filter { it.state == DownloadState.COMPLETED && it.localUri != null && sameRef(it.ref, ref) }.associateBy { it.episode }
    }.distinctUntilChanged()

    // ------------------------------------------------------------------ auto-download

    /** Upserts the rule of `rule.ref` (switch off = `enabled = false`, kept with its last episode); null = no change. */
    override suspend fun setRule(rule: AutoDownloadRule?) {
        if (rule == null) return
        val next = _rules.value.filterNot { sameRef(it.ref, rule.ref) } + rule
        _rules.value = next
        withContext(Dispatchers.IO) { store.saveRules(next) }
    }

    override suspend fun preview(ref: MediaRef): AutoDownloadPreview? = withContext(Dispatchers.IO) {
        val rule = _rules.value.firstOrNull { sameRef(it.ref, ref) } ?: return@withContext null
        val have = records.value.values.map { it.item }.filter { sameRef(it.ref, ref) && it.state != DownloadState.FAILED }.map { it.episode }.toSet()
        val eps = runCatching { metadata.episodes(ref) }.getOrDefault(emptyList())
        val next = DownloadLogic.newEpisodes(rule, eps, have, LocalDate.now())
        val sizes = records.value.values.map { it.item }.filter { sameRef(it.ref, ref) && it.state == DownloadState.COMPLETED }.mapNotNull { it.totalBytes }
        AutoDownloadPreview(rule, next, if (sizes.isEmpty() || next.isEmpty()) null else sizes.average().toLong() * next.size)
    }

    override suspend fun checkNow(): Result<Int> = runCatchingIo {
        val s = settings().sanitized()
        if (DownloadLogic.overLimit(storageUsedBytes(), s.storageLimitGib)) throw AppException(AppError.Extension("", ERR_LIMIT))
        val watching = lists?.let { l ->
            runCatching { l.entries(null).first() }.getOrNull()
                ?.filter { it.status == app.tatakai.next.model.ListStatus.WATCHING || it.status == app.tatakai.next.model.ListStatus.REPEATING }
        }
        var count = 0
        for (rule in _rules.value.filter { it.enabled }) {
            // With a linked list, a rule only runs while the title is still being watched (classic: automation over WATCHING).
            if (!watching.isNullOrEmpty() && watching.none { sameRef(it.ref, rule.ref) }) continue
            val have = records.value.values.map { it.item }.filter { sameRef(it.ref, rule.ref) && it.state != DownloadState.FAILED }.map { it.episode }.toSet()
            val eps = runCatching { metadata.episodes(rule.ref) }.getOrDefault(emptyList())
            val next = DownloadLogic.newEpisodes(rule, eps, have, LocalDate.now())
            var last = rule.lastEpisode
            if (next.isNotEmpty()) {
                val media = runCatching { metadata.media(MediaKind.ANIME, rule.ref) }.getOrNull()
                if (media != null) {
                    for (ep in next) {
                        val r = resolveStream(media, ep, s.preferredServer, s.preferredAudioLang, rule.profile) ?: continue
                        enqueueInternal(DownloadRequest(media, ep, r.first, r.second), foreground = false).onSuccess {
                            count++
                            last = maxOf(last, ep)
                        }
                    }
                }
            }
            setRule(rule.copy(lastEpisode = last, lastCheckedMs = clock()))
        }
        count
    }

    // ------------------------------------------------------------------ local library

    override suspend fun importFiles(uris: List<String>): Result<List<LocalLibraryItem>> = runCatchingIo {
        val added = uris.mapNotNull { u ->
            val uri = Uri.parse(u)
            runCatching { context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
            val doc = DocumentFile.fromSingleUri(context, uri) ?: return@mapNotNull null
            libraryItem(u, doc.name ?: uri.lastPathSegment ?: u, doc.length())
        }
        addToLibrary(added)
    }

    override suspend fun scanFolder(treeUri: String): Result<List<LocalLibraryItem>> = runCatchingIo {
        val uri = Uri.parse(treeUri)
        runCatching { context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        val root = DocumentFile.fromTreeUri(context, uri) ?: throw AppException(AppError.Unknown(ERR_FOLDER))
        val found = ArrayList<LocalLibraryItem>()
        fun walk(dir: DocumentFile, depth: Int) {
            if (depth > MAX_SCAN_DEPTH) return
            for (f in dir.listFiles()) {
                val name = f.name ?: continue
                if (f.isDirectory) walk(f, depth + 1) else if (f.isFile && DownloadLogic.isVideo(name)) found += libraryItemBlocking(f.uri.toString(), name, f.length())
            }
        }
        walk(root, 0)
        addToLibrary(found.map { proposeMatch(it) })
    }

    private suspend fun libraryItem(uri: String, name: String, size: Long): LocalLibraryItem = proposeMatch(libraryItemBlocking(uri, name, size))

    private fun libraryItemBlocking(uri: String, name: String, size: Long): LocalLibraryItem {
        val guess = DownloadLogic.guessLocal(name)
        return LocalLibraryItem(
            id = DownloadLogic.localId(uri),
            uri = uri,
            fileName = name,
            sizeBytes = size,
            title = guess.title,
            episode = guess.episode,
            addedAtMs = clock(),
        )
    }

    /** First metadata hit of the parsed title (the user corrects it with `matchLocal`). */
    private suspend fun proposeMatch(item: LocalLibraryItem): LocalLibraryItem {
        val title = item.title ?: return item
        val hit = runCatching { metadata.search(MediaKind.ANIME, title).items.firstOrNull() }.getOrNull() ?: return item
        return item.copy(ref = hit.ref, title = hit.title, poster = hit.cover)
    }

    private suspend fun addToLibrary(items: List<LocalLibraryItem>): List<LocalLibraryItem> {
        val known = _library.value.map { it.uri }.toSet()
        val fresh = items.filter { it.uri !in known }.distinctBy { it.uri }
        val next = _library.value + fresh
        _library.value = next
        withContext(Dispatchers.IO) { store.saveLibrary(next) }
        return fresh
    }

    override suspend fun matchLocal(id: String, media: Media?, episode: Int?) {
        val next = _library.value.map {
            if (it.id != id) it else it.copy(ref = media?.ref, title = media?.title ?: it.title, poster = media?.cover, episode = episode)
        }
        _library.value = next
        withContext(Dispatchers.IO) { store.saveLibrary(next) }
    }

    override suspend fun removeLocal(id: String, deleteFile: Boolean) {
        val item = _library.value.firstOrNull { it.id == id } ?: return
        if (deleteFile) withContext(Dispatchers.IO) { engine.delete(item.uri) }
        runCatching { context.contentResolver.releasePersistableUriPermission(Uri.parse(item.uri), Intent.FLAG_GRANT_READ_URI_PERMISSION) }
        val next = _library.value.filterNot { it.id == id }
        _library.value = next
        withContext(Dispatchers.IO) { store.saveLibrary(next) }
    }

    override suspend fun storageUsedBytes(): Long = withContext(Dispatchers.IO) {
        val local = DownloadLogic.folderBytes(engine.rootDir)
        val saf = records.value.values.map { it.item }.filter { it.localUri?.startsWith("content://") == true }.sumOf { it.totalBytes ?: 0L }
        local + saf
    }

    override suspend fun verifyIntegrity(): List<DownloadItem> = withContext(Dispatchers.IO) {
        val broken = records.value.values.map { it.item }.filter { item ->
            val uri = item.localUri
            val (exists, length) = if (uri == null) false to 0L else engine.stat(uri)
            DownloadLogic.isBroken(item, exists, length)
        }
        broken.forEach { b -> update(b.id, persist = false) { it.copy(state = DownloadState.BROKEN, error = ERR_BROKEN) } }
        if (broken.isNotEmpty()) mutex.withLock { persist() }
        broken.mapNotNull { records.value[it.id]?.item }
    }

    /** `autoEvictWatched`: deletes completed episodes once their history row says watched. */
    private suspend fun evictWatchedLoop() {
        combine(history.history(EVICT_HISTORY).catch { emit(emptyList()) }, items) { h, list -> DownloadLogic.watchedToEvict(list, h) }
            .distinctUntilChanged()
            .collect { victims -> if (settings().autoEvictWatched) victims.forEach { remove(it.id) } }
    }

    // ------------------------------------------------------------------ reset

    /** "Restablecer la app": removes every download, the index, the library and the rules. */
    override suspend fun wipe() {
        val sent = runCatching { DownloadService.sendRemoveAllDownloads(context, TatakaiDownloadService::class.java, false) }.isSuccess
        if (!sent) withContext(Dispatchers.Main) { runCatching { manager.removeAllDownloads() } }
        records.value = emptyMap()
        hidden.value = emptySet()
        _rules.value = emptyList()
        _library.value = emptyList()
        withContext(Dispatchers.IO) {
            storeDir.deleteRecursively()
            engine.rootDir.listFiles()?.filter { it.name != DownloadEngine.CACHE_DIR }?.forEach { it.deleteRecursively() }
            context.getDatabasePath(DB_NAME).let { db -> db.delete(); File(db.path + "-journal").delete() }
        }
    }

    // ------------------------------------------------------------------ helpers

    private fun update(id: String, persist: Boolean, transform: (DownloadItem) -> DownloadItem) {
        records.update { m -> m[id]?.let { rec -> m + (id to rec.copy(item = transform(rec.item))) } ?: m }
        if (persist) scope.launch { mutex.withLock { persist() } }
    }

    private fun persist() {
        runCatching { store.saveDownloads(records.value.values.toList()) }
    }

    private suspend fun <T> runCatchingIo(block: suspend () -> T): Result<T> = withContext(Dispatchers.IO) {
        try {
            Result.success(block())
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    companion object {
        const val DIR_NAME: String = "downloads"
        const val DB_NAME: String = "downloads.db"

        /** Notification channel shared with [TatakaiDownloadService]. */
        const val CHANNEL_ID: String = "downloads"

        /** Extra of the "download finished" notification tap (read by `MainActivity` through `WaveBLinks`): `DownloadsTab.id`. */
        const val EXTRA_TAB: String = NotificationExtras.DOWNLOADS_TAB

        /** Media3 stop reason of a user pause. */
        internal const val STOP_PAUSED = 1
        private const val PROGRESS_TICK_MS = 1_000L
        private const val PERSIST_EVERY = 10
        private const val MAX_SCAN_DEPTH = 4
        private const val EVICT_HISTORY = 500

        // Short error codes shown in the rows (AppError.Extension msg) — localised by :feature:downloads (downloadErrorText).
        const val ERR_DASH = "dash_not_supported"
        const val ERR_LIMIT = "storage_limit"
        const val ERR_NO_SOURCE = "no_source"
        const val ERR_FAILED = "failed"
        const val ERR_BROKEN = "broken"
        const val ERR_FOLDER = "folder_unavailable"

        internal fun sameRef(a: MediaRef, b: MediaRef): Boolean =
            (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId) ||
                (a.anilistId == null && a.malId == null && a.kitsuId != null && a.kitsuId == b.kitsuId)
    }
}
