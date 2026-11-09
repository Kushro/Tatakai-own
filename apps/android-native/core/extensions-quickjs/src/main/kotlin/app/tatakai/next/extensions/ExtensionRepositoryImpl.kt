// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.AppError
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.Container
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionDetail
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ExtensionLogEntry
import app.tatakai.next.model.ExtensionPrefType
import app.tatakai.next.model.ExtensionPreference
import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.ExtensionUpdate
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ProviderBookmark
import app.tatakai.next.model.ProviderItem
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.parseSourceKey
import app.tatakai.next.model.sourceKey
import app.tatakai.next.release.rank
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.security.MessageDigest
import java.util.Locale
import java.util.UUID
import okhttp3.OkHttpClient

/**
 * Installs `.kai` packages into internal storage (`<root>/<id>/{manifest.json,bundle.js,state.json}`) and resolves streams / ranked
 * source candidates through [ExtensionHost]. [openUri] turns the SAF `content://` URI (or a file path) into a stream; it is injected so the
 * class is testable without Android. [rootDir] must be an app-private directory (the Koin module passes `filesDir/extensions`).
 *
 * android wave (docs/redesign/12 §3.6, §4 e): [resolveCandidates] (web + torrent candidates of every enabled extension, ranked with
 * :core:release `rank` over [SettingsRepository] source prefs and [SourceStatsStore] reliabilities), [reportSourceResult], per-extension
 * [logs], [detail] (package SHA-256, domains, install date), [customHomeRows]. `state.json` = `{ enabled, installedAt, sha256 }`.
 */
class ExtensionRepositoryImpl(
    private val rootDir: File,
    private val openUri: (String) -> InputStream,
    private val host: ExtensionHost,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val settings: SettingsRepository? = null,
    private val stats: SourceStatsStore? = null,
    private val logStore: ExtensionLogStore = ExtensionLogStore(),
    private val clock: () -> Long = System::currentTimeMillis,
    /** docs/redesign/13 §3.10: linked PC extension servers (and any other non-`.kai` provider), ranked together with the extensions. */
    private val external: List<ExternalCandidateSource> = emptyList(),
    // --- parity wave B (16 §3.3 / §3.4, worker t) ---
    /** Downloads of `.kai` packages and repository indexes (the extensions' own client: no app credentials). null = no network installs. */
    private val http: OkHttpClient? = null,
    /** SECRET extension preferences (`SecretKeys.extensionPreference`). null = secrets are not kept. */
    private val secrets: SecretStore? = null,
    /** Where URL installs are downloaded before [installFromUri] (app cache). */
    private val downloadDir: File = File(rootDir, ".downloads"),
    /** Embed pages answered by `custom/watch` are resolved through it (lazy: the sniffer needs the main thread only when used). */
    private val embedSniffer: () -> EmbedSniffer? = { null },
    /** Custom sources of the linked PC extension servers (`server:<serverId>/…`). */
    private val externalCustom: List<ExternalCustomSources> = emptyList(),
    /** Repositories, provider bookmarks, kill-switch snapshot and non-secret preferences. */
    private val catalogFile: File = File(rootDir, CATALOG_FILE),
) : ExtensionRepository {
    private val scope = CoroutineScope(SupervisorJob() + io)
    private val state = MutableStateFlow<List<InstalledExtension>>(emptyList())
    private val records = HashMap<String, Record>()

    /**
     * Raw candidates of a (media, episode) for [CANDIDATE_CACHE_TTL_MS]: Details resolves the panel and the player resolves again when
     * Watch is pressed; extension routes can take a minute, so the second call reuses the first. Only non-empty results are kept (a retry
     * after "no sources" asks the extensions again); the key includes every runnable extension's id and package hash, so installing,
     * updating or toggling one starts fresh. Ranking, reliability and the picked source are recomputed on every call.
     */
    private val candidateCache = LinkedHashMap<String, Pair<Long, List<SourceCandidate>>>()
    private val lock = Mutex()
    private val ready = CompletableDeferred<Unit>()

    // --- parity wave B (worker t) ---
    private val catalogStore = CatalogStore(catalogFile)
    private var catalog = CatalogState()
    private val _repos = MutableStateFlow<List<ExtensionRepo>>(emptyList())
    private val _updates = MutableStateFlow<List<ExtensionUpdate>>(emptyList())
    private val _bookmarks = MutableStateFlow<List<ProviderBookmark>>(emptyList())
    private val _prefValues = MutableStateFlow<Map<String, Map<String, String>>>(emptyMap())
    /** Secret preference keys that hold a value, per extension (the values themselves stay in [secrets]). */
    private val _secretKeysSet = MutableStateFlow<Map<String, Set<String>>>(emptyMap())
    /** Bumped when an extension's preferences change, so cached candidates of the old values are not reused. */
    private val prefVersions = java.util.concurrent.ConcurrentHashMap<String, Int>()

    /** One installed extension: what the host runs plus what the screen shows. */
    private class Record(
        val ext: LoadedExtension,
        val report: CompatReport,
        val torrentAll: Boolean,
        val stored: StoredState,
    ) {
        val manifest: ExtensionManifest get() = ext.manifest
        val runnable: Boolean get() = ext.enabled && ext.compat != CompatLevel.UNSUPPORTED
    }

    private data class StoredState(val enabled: Boolean, val installedAtMs: Long?, val sha256: String?)

    override val installed: StateFlow<List<InstalledExtension>> = state.asStateFlow()

    override val externalSourcesAvailable: Flow<Boolean> =
        if (external.isEmpty()) flowOf(false) else combine(external.map { it.available }) { flags -> flags.any { it } }

    init {
        scope.launch {
            lock.withLock {
                runCatching { loadAllFromDisk() }
                runCatching { loadCatalog() }
                ready.complete(Unit)
            }
            // ext.updates.check: at app start, when Settings allow it, refresh the repositories and publish the updates (badge).
            if (settings?.settings?.value?.extensionUpdateCheck != false && _repos.value.isNotEmpty() && http != null) {
                runCatchingNonCancel {
                    _repos.value.map { it.url }.forEach { url -> refreshRepository(url) }
                    checkUpdates()
                }
            }
        }
    }

    private fun loadAllFromDisk() {
        val dirs = rootDir.listFiles { f -> f.isDirectory && ExtensionManifest.ID_PATTERN.matches(f.name) }.orEmpty()
        for (dir in dirs) {
            try {
                val manifest = ManifestParser.parse(File(dir, "manifest.json").readText())
                if (manifest.id != dir.name) continue
                val bundleFile = File(dir, "bundle.js")
                val bundle = bundleFile.readText()
                val report = CompatAnalyzer.analyze(bundle)
                val stored = readState(dir, default = report.level != CompatLevel.UNSUPPORTED)
                val enabled = stored.enabled && report.level != CompatLevel.UNSUPPORTED
                // Packages installed before the android wave have no stored hash: the bundle's hash stands in for it.
                val filled = stored.copy(
                    enabled = enabled,
                    installedAtMs = stored.installedAtMs ?: bundleFile.lastModified().takeIf { it > 0 },
                    sha256 = stored.sha256 ?: sha256Hex(bundle.toByteArray(Charsets.UTF_8)),
                )
                register(Record(LoadedExtension(manifest, bundle, report.level, enabled), report, Candidates.exposesTorrentAll(bundle), filled))
                logCompat(manifest, report)
            } catch (_: Exception) {
                // A damaged package is skipped; it can be removed by reinstalling.
            }
        }
        publish()
    }

    override suspend fun installFromUri(uri: String): Result<InstalledExtension> = withContext(io) {
        ready.await()
        var id = ""
        try {
            val bytes = openUri(uri).use { readCapped(it) }
            val pkg = KaiReader.read(ByteArrayInputStream(bytes))
            id = pkg.manifest.id
            val sha = sha256Hex(bytes)
            lock.withLock {
                val dir = File(rootDir, id)
                check(dir.canonicalFile.parentFile == rootDir.canonicalFile) { "bad extension directory" }
                dir.mkdirs()
                File(dir, "manifest.json").writeText(pkg.manifestText)
                File(dir, "bundle.js").writeText(pkg.bundle)
                val enabled = pkg.enabledByDefault
                val stored = StoredState(enabled, clock(), sha)
                writeState(dir, stored)
                register(
                    Record(LoadedExtension(pkg.manifest, pkg.bundle, pkg.compat.level, enabled), pkg.compat, Candidates.exposesTorrentAll(pkg.bundle), stored),
                )
                logStore.ok(id, "install", "installed ${pkg.manifest.name} v${pkg.manifest.version} (SHA-256 ${displayHash(sha)})")
                logCompat(pkg.manifest, pkg.compat)
                publish()
                Result.success(state.value.first { it.id == id })
            }
        } catch (e: Exception) {
            val message = when (e) {
                is ManifestException, is KaiException -> e.message.orEmpty()
                else -> "Could not read the extension package"
            }
            if (id.isNotEmpty()) logStore.err(id, "install", message)
            Result.failure(AppException(AppError.Extension(id, message), e))
        }
    }

    override suspend fun setEnabled(id: String, enabled: Boolean) {
        withContext(io) {
            ready.await()
            lock.withLock {
                val cur = records[id] ?: return@withContext
                val effective = enabled && cur.ext.compat != CompatLevel.UNSUPPORTED
                val stored = cur.stored.copy(enabled = effective)
                writeState(File(rootDir, id), stored)
                register(Record(LoadedExtension(cur.manifest, cur.ext.bundle, cur.ext.compat, effective), cur.report, cur.torrentAll, stored))
                logStore.ok(id, "install", if (effective) "enabled" else "disabled")
                publish()
            }
        }
    }

    override suspend fun remove(id: String) {
        withContext(io) {
            ready.await()
            lock.withLock {
                val removed = records.remove(id) ?: return@withContext
                host.unregister(id)
                logStore.clear(id)
                forgetPreferences(id, removed.manifest.preferences)
                val dir = File(rootDir, id)
                // Only a validated-id child of the app-private extensions root is ever deleted.
                if (ExtensionManifest.ID_PATTERN.matches(id) && dir.canonicalFile.parentFile == rootDir.canonicalFile) dir.deleteRecursively()
                publish()
            }
        }
    }

    override suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> {
        ready.await()
        val candidates = lock.withLock {
            records.values.filter { it.runnable && it.manifest.type != ExtensionType.TORRENT && "stream" in it.manifest.enabledAggregateRoutes }
                .sortedBy { it.manifest.priority }
                .map { it.manifest }
        }
        if (candidates.isEmpty()) return emptyList()
        val params = streamParams(media, episode)
        return coroutineScope {
            candidates.map { m ->
                async(io) {
                    // A failing extension must not hide the others: errors become "no sources from it".
                    withTimeoutOrNull(budgetMs(m)) {
                        runCatching { SourceNormalizer.toStreamSources(host.call(m.id, "stream", withPreferences(m, params)), m.id) }
                            .getOrDefault(emptyList())
                    } ?: emptyList()
                }
            }.awaitAll().flatten()
        }
    }

    override suspend fun resolveCandidates(media: Media, episode: Int): List<RankedSource> {
        ready.await()
        // External providers (extension servers) run in parallel with the installed extensions; each bounds its own time.
        val (local, ext) = coroutineScope {
            val remote = async(io) { externalCandidates(media, episode) }
            localCandidates(media, episode) to remote.await()
        }
        val (localRaw, localIds) = local
        val raw = localRaw + ext
        if (raw.isEmpty()) return emptyList()
        val enabledIds = localIds + ext.map { it.extId }
        val withReliability = raw.map { c ->
            val r = stats?.let { s -> runCatchingNonCancel { s.reliability(c.extId, c.label) } }
            if (r == null) c else c.copy(reliability = r)
        }
        val prefs: SourcePrefs = settings?.settings?.value?.sourcePrefs ?: SourcePrefs()
        // sources.embed-sniffer.timeout-setting: with the sniffer switched off, embed pages never reach the selector.
        val visible = if (prefs.allowEmbed) withReliability else withReliability.filterNot { it.isEmbed }
        if (visible.isEmpty()) return emptyList()
        val ranked = rank(visible, prefs) { it in enabledIds }
        val picked = HistoryEntry.animeIdOf(media.ref.anilistId, media.ref.malId)?.let { mediaId ->
            stats?.let { s -> runCatchingNonCancel { s.pickedSource(mediaId, episode) } }
        }
        return ranked.mapIndexed { i, (candidate, score) ->
            RankedSource(
                candidate = candidate,
                score = score,
                extEnabled = candidate.extId in enabledIds,
                recommended = i == 0,
                picked = picked != null && sourceKey(candidate) == picked,
            )
        }
    }

    /** Raw candidates of the installed, runnable extensions (cached, see [candidateCache]) and their ids. */
    private suspend fun localCandidates(media: Media, episode: Int): Pair<List<SourceCandidate>, Set<String>> {
        var cacheKey = ""
        val snapshot: List<Pair<ExtensionManifest, List<String>>> = lock.withLock {
            val runnable = records.values.filter { it.runnable }
                .sortedWith(compareBy<Record> { it.manifest.priority }.thenBy { it.manifest.id })
            cacheKey = "${media.ref}|$episode|" + runnable.joinToString(",") { "${it.manifest.id}@${it.stored.sha256}#${prefsVersion(it.manifest.id)}" }
            runnable.map { it.manifest to Candidates.candidateRoutes(it.manifest, it.torrentAll) }
                .filter { it.second.isNotEmpty() }
        }
        if (snapshot.isEmpty()) return emptyList<SourceCandidate>() to emptySet()
        val enabledIds = snapshot.map { it.first.id }.toSet()
        val cached = synchronized(candidateCache) {
            candidateCache[cacheKey]?.takeIf { clock() - it.first < CANDIDATE_CACHE_TTL_MS }?.second
        }
        val raw: List<SourceCandidate> = cached ?: run {
            val params = streamParams(media, episode)
            coroutineScope {
                snapshot.flatMap { (m, routes) ->
                    // Routes of one extension are serialized by the host, so each one may wait for the others.
                    val budget = budgetMs(m) * routes.size
                    val withPrefs = withPreferences(m, params)
                    routes.map { route -> async(io) { candidatesOf(m, route, withPrefs, budget) } }
                }.awaitAll().flatten()
            }.also { fresh ->
                if (fresh.isNotEmpty()) synchronized(candidateCache) {
                    candidateCache[cacheKey] = clock() to fresh
                    while (candidateCache.size > CANDIDATE_CACHE_MAX) candidateCache.remove(candidateCache.keys.first())
                }
            }
        }
        return raw to enabledIds
    }

    /** Candidates of every [external] source; a failing source contributes nothing (it never hides the extensions). */
    private suspend fun externalCandidates(media: Media, episode: Int): List<SourceCandidate> {
        if (external.isEmpty()) return emptyList()
        return coroutineScope {
            external.map { source -> async(io) { runCatchingNonCancel { source.candidates(media, episode) }.orEmpty() } }
                .awaitAll().flatten()
        }
    }

    private suspend fun candidatesOf(m: ExtensionManifest, route: String, params: JsonObject, budget: Long): List<SourceCandidate> {
        val result: Result<List<SourceCandidate>>? = withTimeoutOrNull(budget) {
            try {
                Result.success(Candidates.mapCandidates(m.id, m.name, host.call(m.id, route, params)))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
        }
        return when {
            result == null -> {
                logStore.err(m.id, route, "no answer within ${budget / 1000} s")
                emptyList()
            }
            result.isFailure -> {
                logStore.err(m.id, route, result.exceptionOrNull()?.message ?: "source listing failed")
                emptyList()
            }
            else -> result.getOrThrow().also { list ->
                if (list.isEmpty()) logStore.warn(m.id, route, "no sources for this episode") else logStore.ok(m.id, route, "${list.size} sources")
            }
        }
    }

    override suspend fun reportSourceResult(sourceKey: String, ok: Boolean) {
        val (extId, kind, label) = parseSourceKey(sourceKey) ?: return
        val updated = stats?.let { s -> runCatchingNonCancel { s.recordSourceResult(extId, label, ok) } }
        val rel = updated?.let { " (reliability ${String.format(Locale.ROOT, "%.2f", it)})" }.orEmpty()
        if (ok) {
            logStore.ok(extId, "source", "${kind.id} source '$label' played$rel")
        } else {
            logStore.warn(extId, "source", "${kind.id} source '$label' failed$rel")
        }
    }

    override fun logs(extensionId: String): Flow<List<ExtensionLogEntry>> = logStore.flow(extensionId)

    override suspend fun detail(extensionId: String): ExtensionDetail? {
        ready.await()
        val r = lock.withLock { records[extensionId] } ?: return null
        val m = r.manifest
        return ExtensionDetail(
            id = m.id,
            domains = m.permissions.filter { it.startsWith(NetworkPolicy.PREFIX) }
                .map { it.removePrefix(NetworkPolicy.PREFIX).trim().lowercase(Locale.ROOT) }.filter { it.isNotEmpty() }.distinct(),
            hash = r.stored.sha256?.let(::displayHash).orEmpty(),
            installedAtMs = r.stored.installedAtMs,
            kind = m.type.wire,
            browserDomains = BrowserPolicy.browserDomains(m),
            sourceRoutes = Candidates.candidateRoutes(m, r.torrentAll),
        )
    }

    override suspend fun customHomeRows(): List<ExtensionHomeRow> {
        ready.await()
        val targets = lock.withLock {
            records.values.filter { it.runnable && it.manifest.customSources.isNotEmpty() }
                .sortedWith(compareBy<Record> { it.manifest.priority }.thenBy { it.manifest.id })
                .flatMap { r -> r.manifest.customSources.filter { it.kind == "watch" }.map { r.manifest to it } }
        }
        if (targets.isEmpty()) return emptyList()
        return coroutineScope {
            targets.map { (m, src) ->
                async(io) {
                    val body = withTimeoutOrNull(budgetMs(m)) {
                        try {
                            host.call(m.id, "custom/home", withPreferences(m, buildJsonObject { put("sourceId", src.id) }))
                        } catch (e: CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            logStore.err(m.id, "home", e.message ?: "custom home failed")
                            null
                        }
                    }
                    body?.let { homeRows(m, src, it) }.orEmpty()
                }
            }.awaitAll().flatten()
        }
    }

    /**
     * Parity wave A (15 §3.1, worker o): Search › extension chip. Runs `custom/search` on every `watch` custom source of [extensionId]
     * (page 1) and merges the cards; an extension without custom sources (or not runnable) returns nothing. Tapping a card searches its
     * title in the metadata provider, like the Home extension rows.
     */
    override suspend fun searchExtension(extensionId: String, query: String): List<ExtensionHomeItem> {
        val q = query.trim()
        if (q.isEmpty()) return emptyList()
        ready.await()
        val (manifest, sources) = lock.withLock {
            val r = records[extensionId]?.takeIf { it.runnable } ?: return emptyList()
            r.manifest to r.manifest.customSources.filter { it.kind == ExtensionSearch.WATCH_KIND }
        }
        if (sources.isEmpty()) return emptyList()
        val perSource = coroutineScope {
            sources.map { src ->
                async(io) {
                    withTimeoutOrNull(budgetMs(manifest)) {
                        try {
                            ExtensionSearch.cards(
                                host.call(
                                    manifest.id,
                                    "custom/search",
                                    withPreferences(
                                        manifest,
                                        buildJsonObject {
                                            put("sourceId", src.id)
                                            put("query", q)
                                            put("page", 1)
                                        },
                                    ),
                                ),
                            )
                        } catch (e: CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            logStore.err(manifest.id, "search", e.message ?: "custom search failed")
                            null
                        }
                    }.orEmpty()
                }
            }.awaitAll()
        }
        return ExtensionSearch.merge(perSource)
    }

    private fun homeRows(m: ExtensionManifest, src: CustomSourceDescriptor, body: JsonObject): List<ExtensionHomeRow> =
        (body["sections"] as? JsonArray).orEmpty().mapNotNull { section ->
            val o = section as? JsonObject ?: return@mapNotNull null
            val items = (o["items"] as? JsonArray).orEmpty().mapNotNull { item ->
                val card = item as? JsonObject ?: return@mapNotNull null
                val title = card.text("title") ?: return@mapNotNull null
                ExtensionHomeItem(id = card.text("id") ?: title, title = title, poster = card.text("image"))
            }
            if (items.isEmpty()) null else ExtensionHomeRow(m.id, src.id, o.text("title") ?: src.name, items)
        }

    // ---- parity wave B (16 §3.4, worker t): install from URL, updates, preferences, repositories, providers, kill switch ---------

    /** ext.install-url: a `.kai` URL, or a `manifest.json` whose `package` / `url` / `download` (or the sibling `<id>.kai`) is the package. */
    override suspend fun installFromUrl(url: String): Result<InstalledExtension> {
        val client = http ?: return Result.failure(AppException(AppError.Unknown("network installs unavailable")))
        val trimmed = url.trim()
        return try {
            val packageUrl = withContext(io) { packageUrlOf(client, trimmed) }
            downloadAndInstall(client, packageUrl)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(AppException(AppError.Extension("", e.message ?: "Could not download the extension"), e))
        }
    }

    /** `.kai` -> itself; a manifest -> its package URL; a repository index -> error (it is added as a repository instead). */
    private fun packageUrlOf(client: OkHttpClient, url: String): String {
        val path = url.substringBefore('?').substringBefore('#').lowercase(Locale.ROOT)
        if (path.endsWith(".kai")) return RepoIndex.resolve(url, url) ?: throw IOException("Invalid URL")
        val text = httpGetBytes(client, url, MAX_INDEX_BYTES, "application/json").decodeToString()
        val o = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull()
            ?: throw IOException("The URL is neither a .kai package nor a manifest")
        if (o["extensions"] is JsonArray) throw IOException("This URL is an extension repository: add it under Repositories")
        val explicit = listOf("package", "packageUrl", "kai", "download", "url").firstNotNullOfOrNull { k -> o.text(k) }
        if (explicit != null) return RepoIndex.resolve(url, explicit) ?: throw IOException("Invalid package URL in the manifest")
        val id = o.text("id")?.takeIf { ExtensionManifest.ID_PATTERN.matches(it) } ?: throw IOException("The manifest has no valid id")
        return RepoIndex.resolve(url, "$id.kai") ?: throw IOException("Invalid URL")
    }

    private suspend fun downloadAndInstall(client: OkHttpClient, packageUrl: String): Result<InstalledExtension> {
        val file = withContext(io) {
            downloadDir.mkdirs()
            val bytes = httpGetBytes(client, packageUrl, KaiReader.MAX_TOTAL_BYTES.toLong(), null)
            File(downloadDir, "${UUID.randomUUID()}.kai").apply { writeBytes(bytes) }
        }
        return try {
            installFromUri(file.path)
        } finally {
            withContext(io) { file.delete() }
        }
    }

    override val updates: StateFlow<List<ExtensionUpdate>> get() = _updates.asStateFlow()

    override suspend fun checkUpdates(): List<ExtensionUpdate> {
        ready.await()
        val notes = lock.withLock { catalog.repos.flatMap { r -> r.entries.map { "${r.url}|${it.id}" to it.notes } }.toMap() }
        val found = findUpdates(state.value, _repos.value, notes)
        _updates.value = found
        found.forEach { u -> logStore.ok(u.id, "install", "update available: v${u.installedVersion} -> v${u.availableVersion}") }
        return found
    }

    /** Keeps the enabled flag (preferences live outside the package, so they survive). */
    override suspend fun update(extensionId: String): Result<InstalledExtension> {
        val client = http ?: return Result.failure(AppException(AppError.Unknown("network installs unavailable")))
        ready.await()
        val upd = _updates.value.firstOrNull { it.id == extensionId } ?: checkUpdates().firstOrNull { it.id == extensionId }
            ?: return Result.failure(AppException(AppError.Extension(extensionId, "No update available")))
        val wasEnabled = state.value.firstOrNull { it.id == extensionId }?.enabled
        val result = try {
            downloadAndInstall(client, upd.url)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(AppException(AppError.Extension(extensionId, e.message ?: "Could not download the update"), e))
        }
        val installed = result.getOrNull() ?: return result
        if (installed.id != extensionId) {
            return Result.failure(AppException(AppError.Extension(extensionId, "The package has another id (${installed.id})")))
        }
        if (wasEnabled == false && installed.enabled) setEnabled(extensionId, false)
        _updates.value = _updates.value.filterNot { it.id == extensionId && !Versions.isNewer(it.availableVersion, installed.version) }
        repoEntriesChanged()
        return Result.success(state.value.firstOrNull { it.id == extensionId } ?: installed)
    }

    override suspend fun preferences(extensionId: String): List<ExtensionPreference> {
        ready.await()
        val m = lock.withLock { records[extensionId]?.manifest } ?: return emptyList()
        refreshSecretKeys(m)
        return m.preferences
    }

    /** Non-secret values as stored; a SECRET key with a stored value maps to [SECRET_SET] (the value itself is never exposed). */
    override fun preferenceValues(extensionId: String): Flow<Map<String, String>> =
        combine(_prefValues, _secretKeysSet) { plain, secret ->
            plain[extensionId].orEmpty() + secret[extensionId].orEmpty().associateWith { SECRET_SET }
        }

    override suspend fun setPreference(extensionId: String, key: String, value: String?) {
        ready.await()
        val pref = lock.withLock { records[extensionId]?.manifest?.preferences?.firstOrNull { it.key == key } } ?: return
        val clean = value?.takeIf { it.isNotEmpty() }?.take(MAX_PREF_CHARS)
        if (pref.type == ExtensionPrefType.SECRET) {
            val store = secrets ?: return
            store.put(SecretKeys.extensionPreference(extensionId, key), clean)
            _secretKeysSet.value = _secretKeysSet.value.let { m ->
                val cur = m[extensionId].orEmpty()
                m + (extensionId to if (clean == null) cur - key else cur + key)
            }
        } else {
            withContext(io) {
                lock.withLock {
                    val cur = catalog.preferences[extensionId].orEmpty()
                    val next = if (clean == null) cur - key else cur + (key to clean)
                    catalog = catalog.copy(preferences = if (next.isEmpty()) catalog.preferences - extensionId else catalog.preferences + (extensionId to next))
                    catalogStore.write(catalog)
                    _prefValues.value = catalog.preferences
                }
            }
        }
        bumpPrefs(extensionId)
        logStore.ok(extensionId, "install", "preference '$key' ${if (clean == null) "cleared" else "saved"}")
    }

    override val repositories: StateFlow<List<ExtensionRepo>> get() = _repos.asStateFlow()

    override suspend fun addRepository(url: String): Result<ExtensionRepo> {
        val clean = RepoIndex.resolve(url.trim(), url.trim())
            ?: return Result.failure(AppException(AppError.Unknown("Not an http(s) URL")))
        ready.await()
        if (_repos.value.any { it.url == clean }) return refreshRepository(clean)
        return fetchRepository(clean, add = true)
    }

    override suspend fun removeRepository(url: String) {
        ready.await()
        withContext(io) {
            lock.withLock {
                catalog = catalog.copy(repos = catalog.repos.filterNot { it.url == url })
                catalogStore.write(catalog)
            }
        }
        repoEntriesChanged()
        _updates.value = _updates.value.filterNot { it.repositoryUrl == url }
    }

    override suspend fun refreshRepository(url: String): Result<ExtensionRepo> {
        ready.await()
        return fetchRepository(url, add = false)
    }

    private suspend fun fetchRepository(url: String, add: Boolean): Result<ExtensionRepo> {
        val client = http ?: return Result.failure(AppException(AppError.Unknown("network unavailable")))
        val fetched: Result<RepoIndex.Parsed> = try {
            Result.success(withContext(io) { RepoIndex.parse(httpGetBytes(client, url, MAX_INDEX_BYTES, "application/json").decodeToString(), url) })
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
        val stored: StoredRepo = withContext(io) {
            lock.withLock {
                val previous = catalog.repos.firstOrNull { it.url == url }
                val next = fetched.fold(
                    onSuccess = { p ->
                        StoredRepo(
                            url = url,
                            name = p.name,
                            entries = p.entries.map { (e, notes) -> StoredRepoEntry(e.id, e.name, e.version, e.description, e.url, notes) },
                            fetchedAtMs = clock(),
                        )
                    },
                    onFailure = { e ->
                        (previous ?: StoredRepo(url = url, name = RepoIndex.hostName(url))).copy(error = e.message ?: "unreachable")
                    },
                )
                // A failing first add is not stored: the user sees the error and can retry.
                if (previous != null || (add && fetched.isSuccess)) {
                    catalog = catalog.copy(repos = catalog.repos.filterNot { it.url == url } + next)
                    catalogStore.write(catalog)
                }
                next
            }
        }
        repoEntriesChanged()
        return fetched.fold(
            onSuccess = { Result.success(_repos.value.firstOrNull { it.url == url } ?: stored.toModel()) },
            onFailure = { e -> Result.failure(AppException(AppError.Unknown(e.message ?: "repository unavailable"), e)) },
        )
    }

    /** Republishes the repositories with the installed / update flags of the current install list. */
    private suspend fun repoEntriesChanged() {
        val repos = lock.withLock { catalog.repos.map { it.toModel() } }
        val installedNow = state.value
        _repos.value = repos.map { r -> r.copy(entries = markInstalled(r.entries, installedNow)) }
    }

    override suspend fun customSources(kind: String?): List<CustomSourceRef> {
        ready.await()
        val local = lock.withLock {
            records.values.filter { it.runnable }
                .sortedWith(compareBy<Record> { it.manifest.priority }.thenBy { it.manifest.id })
                .flatMap { r ->
                    r.manifest.customSources.map { src ->
                        CustomSourceRef(r.manifest.id, r.manifest.name, src.id, src.kind, src.name, src.icon, src.description)
                    }
                }
        }
        // ext.server-client.fallback: a server that is down contributes nothing; the local sources stay.
        val remote = externalCustom.flatMap { group -> runCatchingNonCancel { group.customSources() }.orEmpty() }
        return (local + remote).filter { kind == null || it.kind == kind }
    }

    override suspend fun providerHome(extensionId: String, sourceId: String): List<ExtensionHomeRow> {
        externalCustom.firstOrNull { it.owns(extensionId) }?.let { return it.home(extensionId, sourceId) }
        val (m, src) = customSource(extensionId, sourceId)
        val body = callProvider(m, "custom/home", buildJsonObject { put("sourceId", src.id) }, "home")
        return ProviderParsers.homeRows(m.id, src.id, src.name, body)
    }

    override suspend fun providerSearch(extensionId: String, sourceId: String, query: String, page: Int): List<ExtensionHomeItem> {
        val q = query.trim()
        if (q.isEmpty()) return emptyList()
        externalCustom.firstOrNull { it.owns(extensionId) }?.let { return it.search(extensionId, sourceId, q, page) }
        val (m, src) = customSource(extensionId, sourceId)
        val body = callProvider(
            m, "custom/search",
            buildJsonObject {
                put("sourceId", src.id)
                put("query", q)
                put("page", page.coerceAtLeast(1))
            },
            "search",
        )
        return ProviderParsers.cards(body)
    }

    override suspend fun providerInfo(extensionId: String, sourceId: String, itemId: String): ProviderItem? {
        externalCustom.firstOrNull { it.owns(extensionId) }?.let { return it.info(extensionId, sourceId, itemId) }
        val (m, src) = customSource(extensionId, sourceId)
        val body = callProvider(m, "custom/info", buildJsonObject { put("sourceId", src.id); put("id", itemId) }, "info")
        val item = ProviderParsers.info(itemId, src.kind, body) ?: return null
        return item.copy(match = matchOf(body))
    }

    /** `anilistId` / `malId` the bundle may put in its info answer (or under `meta`): Watch then records history under a real id. */
    private fun matchOf(body: JsonObject): MediaRef? {
        fun idIn(o: JsonObject?, k: String): Int? = (o?.get(k) as? JsonPrimitive)?.let { p -> p.intOrNull ?: p.content.toIntOrNull() }?.takeIf { it > 0 }
        val meta = body["meta"] as? JsonObject
        val al = idIn(body, "anilistId") ?: idIn(meta, "anilistId")
        val mal = idIn(body, "malId") ?: idIn(meta, "malId")
        return if (al == null && mal == null) null else MediaRef(anilistId = al, malId = mal)
    }

    override suspend fun providerWatch(extensionId: String, sourceId: String, itemId: String, entryId: String): List<StreamSource> {
        externalCustom.firstOrNull { it.owns(extensionId) }?.let { return it.watch(extensionId, sourceId, itemId, entryId) }
        val (m, src) = customSource(extensionId, sourceId)
        val body = callProvider(
            m, "custom/watch",
            buildJsonObject {
                put("sourceId", src.id)
                put("id", itemId)
                put("episodeId", entryId)
            },
            "watch",
        )
        val direct = SourceNormalizer.toStreamSources(body, m.id)
        if (direct.isNotEmpty()) return direct
        return sniffEmbeds(m, body)
    }

    /** `custom/watch` answered only embed pages: the first ones go through the sniffer (when allowed in Settings). */
    private suspend fun sniffEmbeds(m: ExtensionManifest, body: JsonObject): List<StreamSource> {
        val prefs = settings?.settings?.value?.sourcePrefs ?: SourcePrefs()
        if (!prefs.allowEmbed) return emptyList()
        val sniffer = embedSniffer() ?: return emptyList()
        val embeds = Candidates.mapCandidates(m.id, m.name, body).filter { it.isEmbed }.take(MAX_PROVIDER_EMBEDS)
        for (c in embeds) {
            val web = c.web ?: continue
            try {
                val capture = sniffer.sniff(web.url, web.headers, m.id, prefs.embedTimeoutSecs * 1000L)
                return listOf(capture.toStream(c))
            } catch (e: EmbedSniffException) {
                logStore.warn(m.id, "embed", "provider embed failed: ${e.reason}")
            }
        }
        return emptyList()
    }

    override suspend fun providerRead(extensionId: String, sourceId: String, itemId: String, entryId: String): List<ChapterPage> {
        externalCustom.firstOrNull { it.owns(extensionId) }?.let { return it.read(extensionId, sourceId, itemId, entryId) }
        val (m, src) = customSource(extensionId, sourceId)
        val body = callProvider(
            m, "custom/read",
            buildJsonObject {
                put("sourceId", src.id)
                put("id", itemId)
                put("chapterId", entryId)
            },
            "read",
        )
        return ProviderParsers.pages(src.kind, body)
    }

    override fun providerBookmarks(): Flow<List<ProviderBookmark>> = _bookmarks.asStateFlow()

    override suspend fun setProviderBookmark(bookmark: ProviderBookmark) {
        ready.await()
        withContext(io) {
            lock.withLock {
                val rest = catalog.bookmarks.filterNot {
                    it.extensionId == bookmark.extensionId && it.sourceId == bookmark.sourceId && it.itemId == bookmark.itemId
                }
                catalog = catalog.copy(bookmarks = listOf(bookmark.toStored()) + rest)
                catalogStore.write(catalog)
                _bookmarks.value = catalog.bookmarks.map { it.toModel() }
            }
        }
    }

    override suspend fun removeProviderBookmark(extensionId: String, sourceId: String, itemId: String) {
        ready.await()
        withContext(io) {
            lock.withLock {
                catalog = catalog.copy(
                    bookmarks = catalog.bookmarks.filterNot { it.extensionId == extensionId && it.sourceId == sourceId && it.itemId == itemId },
                )
                catalogStore.write(catalog)
                _bookmarks.value = catalog.bookmarks.map { it.toModel() }
            }
        }
    }

    /**
     * ext.kill-switch: "Desactivar todas" remembers which extensions were enabled and disables every one; "Activar todas" restores exactly
     * those (or, without a snapshot, enables every supported extension). Packages and preferences are untouched.
     */
    override suspend fun setAllEnabled(enabled: Boolean) {
        ready.await()
        val targets: Map<String, Boolean> = withContext(io) {
            lock.withLock {
                if (!enabled) {
                    val wasOn = records.values.filter { it.ext.enabled }.map { it.manifest.id }
                    if (wasOn.isNotEmpty() || catalog.killSwitchPrevious == null) {
                        catalog = catalog.copy(killSwitchPrevious = wasOn)
                        catalogStore.write(catalog)
                    }
                    records.keys.associateWith { false }
                } else {
                    val previous = catalog.killSwitchPrevious
                    catalog = catalog.copy(killSwitchPrevious = null)
                    catalogStore.write(catalog)
                    records.keys.associateWith { id -> previous == null || id in previous }
                }
            }
        }
        targets.forEach { (id, on) -> setEnabled(id, on) }
    }

    /** True while "Desactivar todas" is in effect (a snapshot to restore exists). */
    val killSwitchOn: Flow<Boolean> get() = state.map { list -> list.isNotEmpty() && list.none { it.enabled } && catalog.killSwitchPrevious != null }

    /** "Restablecer la app": repositories, bookmarks, kill-switch snapshot and preferences (secrets are cleared by the reset itself). */
    suspend fun wipeCatalog() {
        withContext(io) {
            lock.withLock {
                catalog = CatalogState()
                catalogStore.delete()
                _repos.value = emptyList()
                _updates.value = emptyList()
                _bookmarks.value = emptyList()
                _prefValues.value = emptyMap()
                _secretKeysSet.value = emptyMap()
                downloadDir.deleteRecursively()
            }
        }
    }

    // ---- wave B plumbing -----------------------------------------------------------------------------------------------------

    private fun loadCatalog() {
        catalog = catalogStore.read()
        _bookmarks.value = catalog.bookmarks.map { it.toModel() }
        _prefValues.value = catalog.preferences
        val installedNow = state.value
        _repos.value = catalog.repos.map { r -> r.toModel().let { it.copy(entries = markInstalled(it.entries, installedNow)) } }
    }

    /** Which SECRET keys hold a value (read once per extension, lazily, off the lock). */
    private suspend fun refreshSecretKeys(m: ExtensionManifest) {
        val store = secrets ?: return
        val keys = m.preferences.filter { it.type == ExtensionPrefType.SECRET }.mapNotNull { p ->
            p.key.takeIf { runCatchingNonCancel { store.get(SecretKeys.extensionPreference(m.id, p.key)) } != null }
        }.toSet()
        _secretKeysSet.value = _secretKeysSet.value + (m.id to keys)
    }

    /** [params] + the extension's preference values (`preferences` param read by [ExtensionHost]); defaults fill unset keys. */
    private suspend fun withPreferences(m: ExtensionManifest, params: JsonObject): JsonObject {
        if (m.preferences.isEmpty()) return params
        val plain = _prefValues.value[m.id].orEmpty()
        val values = buildJsonObject {
            for (p in m.preferences) {
                val v = when (p.type) {
                    ExtensionPrefType.SECRET -> secrets?.let { s -> runCatchingNonCancel { s.get(SecretKeys.extensionPreference(m.id, p.key)) } }
                    else -> plain[p.key] ?: p.default
                }
                if (v != null) {
                    if (p.type == ExtensionPrefType.TOGGLE) put(p.key, v.equals("true", ignoreCase = true)) else put(p.key, v)
                }
            }
        }
        if (m.id !in _secretKeysSet.value) refreshSecretKeys(m)
        return JsonObject(params + (ExtensionHost.PREFERENCES_PARAM to values))
    }

    private suspend fun forgetPreferences(id: String, prefs: List<ExtensionPreference>) {
        if (catalog.preferences.containsKey(id)) {
            catalog = catalog.copy(preferences = catalog.preferences - id)
            catalogStore.write(catalog)
            _prefValues.value = catalog.preferences
        }
        val store = secrets
        if (store != null) prefs.filter { it.type == ExtensionPrefType.SECRET }.forEach { p ->
            runCatchingNonCancel { store.put(SecretKeys.extensionPreference(id, p.key), null) }
        }
        _secretKeysSet.value = _secretKeysSet.value - id
        bumpPrefs(id)
    }

    private fun prefsVersion(id: String): Int = prefVersions[id] ?: 0

    private fun bumpPrefs(id: String) {
        prefVersions.merge(id, 1) { a, b -> a + b }
    }

    /** The runnable extension and its custom source. @throws AppException when unknown / disabled. */
    private suspend fun customSource(extensionId: String, sourceId: String): Pair<ExtensionManifest, CustomSourceDescriptor> {
        ready.await()
        val r = lock.withLock { records[extensionId] }
            ?: throw AppException(AppError.Extension(extensionId, "Extension not installed"))
        if (!r.runnable) throw AppException(AppError.Extension(extensionId, "Extension disabled"))
        val src = r.manifest.customSources.firstOrNull { it.id == sourceId }
            ?: throw AppException(AppError.Extension(extensionId, "Unknown provider $sourceId"))
        return r.manifest to src
    }

    /** One custom route with the preference values, bounded by the extension budget; failures are logged and rethrown as [AppException]. */
    private suspend fun callProvider(m: ExtensionManifest, route: String, params: JsonObject, op: String): JsonObject {
        val withPrefs = withPreferences(m, params)
        val result = withTimeoutOrNull(budgetMs(m)) {
            try {
                Result.success(host.call(m.id, route, withPrefs))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
        }
        return when {
            result == null -> {
                logStore.err(m.id, op, "no answer within ${budgetMs(m) / 1000} s")
                throw AppException(AppError.Extension(m.id, "timeout"))
            }
            result.isFailure -> {
                val msg = result.exceptionOrNull()?.message ?: "$route failed"
                logStore.err(m.id, op, msg)
                throw AppException(AppError.Extension(m.id, msg), result.exceptionOrNull())
            }
            else -> result.getOrThrow().also { logStore.ok(m.id, op, "$route answered") }
        }
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private fun streamParams(media: Media, episode: Int): JsonObject {
        val titles = (listOf(media.title) + media.titles.values).filter { it.isNotBlank() }.distinct()
        return buildJsonObject {
            put("anilistId", media.ref.anilistId ?: 0)
            put("titles", JsonArray(titles.map { JsonPrimitive(it) }))
            put("episode", episode)
            media.episodes?.let { put("episodeCount", it) }
            put("stream", "0")
        }
    }

    /** A call of an extension that may open the hidden browser can take [BrowserPolicy.CALL_BUDGET_WITH_BROWSER_MS]. */
    private fun budgetMs(m: ExtensionManifest): Long =
        if (BrowserPolicy.browserDomains(m).isNotEmpty()) BrowserPolicy.CALL_BUDGET_WITH_BROWSER_MS + 5_000 else EXTENSION_BUDGET_MS

    private fun logCompat(m: ExtensionManifest, report: CompatReport) {
        when (report.level) {
            CompatLevel.FULL -> Unit
            CompatLevel.PARTIAL -> logStore.warn(m.id, "compat", report.reason ?: "partially compatible")
            CompatLevel.UNSUPPORTED -> logStore.err(m.id, "compat", report.reason ?: "not supported by the native runtime")
        }
        if ((m.permissions + m.capabilities).any { it == "torrent:session" }) {
            logStore.warn(
                m.id, "compat",
                "torrent:session accepted; torrents are listed and played by the app's torrent engine, the bundle gets no session of its own",
            )
        }
        val domains = BrowserPolicy.browserDomains(m)
        if (domains.isNotEmpty()) logStore.ok(m.id, "browser", "hidden browser allowed for ${domains.size} domains")
        if (m.browserDomains.size > domains.size) {
            logStore.warn(m.id, "browser", "${m.browserDomains.size - domains.size} browserDomains ignored: not covered by a network permission or invalid")
        }
    }

    private fun register(record: Record) {
        records[record.manifest.id] = record
        host.register(record.ext)
    }

    private fun publish() {
        state.value = records.values.sortedBy { it.manifest.name.lowercase(Locale.ROOT) }.map {
            InstalledExtension(
                id = it.manifest.id,
                name = it.manifest.name,
                version = it.manifest.version,
                type = it.manifest.type.wire,
                permissions = it.manifest.permissions,
                compat = when (it.ext.compat) {
                    CompatLevel.FULL -> ExtensionCompat.FULL
                    CompatLevel.PARTIAL -> ExtensionCompat.PARTIAL
                    CompatLevel.UNSUPPORTED -> ExtensionCompat.UNSUPPORTED
                },
                enabled = it.ext.enabled,
                compatReason = it.report.reason,
                description = it.manifest.description,
                browserDomains = BrowserPolicy.browserDomains(it.manifest),
            )
        }
        // parity wave B: repository rows show "Installed" / "Update" against the new install list.
        val installedNow = state.value
        _repos.value = catalog.repos.map { r -> r.toModel().let { m -> m.copy(entries = markInstalled(m.entries, installedNow)) } }
    }

    private fun readState(dir: File, default: Boolean): StoredState {
        val o = runCatching { Json.parseToJsonElement(File(dir, "state.json").readText()).jsonObject }.getOrNull()
            ?: return StoredState(default, null, null)
        return StoredState(
            enabled = (o["enabled"] as? JsonPrimitive)?.booleanOrNull ?: default,
            installedAtMs = (o["installedAt"] as? JsonPrimitive)?.longOrNull,
            sha256 = (o["sha256"] as? JsonPrimitive)?.takeIf { it.isString }?.content?.takeIf { SHA_HEX.matches(it) },
        )
    }

    private fun writeState(dir: File, s: StoredState) {
        dir.mkdirs()
        File(dir, "state.json").writeText(
            buildJsonObject {
                put("enabled", s.enabled)
                s.installedAtMs?.let { put("installedAt", it) }
                s.sha256?.let { put("sha256", it) }
            }.toString(),
        )
    }

    private fun readCapped(input: InputStream): ByteArray {
        val out = ByteArrayOutputStream()
        val buf = ByteArray(64 * 1024)
        var total = 0L
        while (true) {
            val n = input.read(buf)
            if (n < 0) break
            total += n
            if (total > KaiReader.MAX_TOTAL_BYTES) throw KaiException("Package is larger than ${KaiReader.MAX_TOTAL_BYTES} bytes")
            out.write(buf, 0, n)
        }
        return out.toByteArray()
    }

    private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? =
        try {
            block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }

    private fun JsonObject.text(key: String): String? =
        (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }

    companion object {
        const val EXTENSION_BUDGET_MS = 25_000L
        /** parity wave B: catalog file (repositories, bookmarks, kill switch, preferences) inside the extensions root. */
        const val CATALOG_FILE = ".catalog.json"
        /** [preferenceValues] value of a SECRET key that holds something (the secret itself never leaves SecretStore). */
        const val SECRET_SET = "\u2022\u2022\u2022\u2022"
        private const val MAX_INDEX_BYTES = 2L * 1024 * 1024
        private const val MAX_PREF_CHARS = 4096
        private const val MAX_PROVIDER_EMBEDS = 2
        /** How long Details' resolved candidates serve the player's own resolve (see candidateCache). */
        const val CANDIDATE_CACHE_TTL_MS = 3 * 60_000L
        private const val CANDIDATE_CACHE_MAX = 16
        private val SHA_HEX = Regex("^[0-9a-f]{64}$")

        internal fun sha256Hex(bytes: ByteArray): String =
            MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { String.format(Locale.ROOT, "%02x", it.toInt() and 0xff) }

        /** `9f2c…b71e` (ExtensionDetail.hash display form). */
        internal fun displayHash(hex: String): String = if (hex.length <= 8) hex else "${hex.take(4)}…${hex.takeLast(4)}"
    }
}
