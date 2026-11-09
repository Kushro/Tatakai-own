// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionDetail
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ExtensionLogEntry
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.RankedSource
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
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.InputStream
import java.security.MessageDigest
import java.util.Locale

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
                ready.complete(Unit)
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
                if (records.remove(id) == null) return@withContext
                host.unregister(id)
                logStore.clear(id)
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
                        runCatching { SourceNormalizer.toStreamSources(host.call(m.id, "stream", params), m.id) }
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
        val ranked = rank(withReliability, prefs) { it in enabledIds }
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
            cacheKey = "${media.ref}|$episode|" + runnable.joinToString(",") { "${it.manifest.id}@${it.stored.sha256}" }
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
                    routes.map { route -> async(io) { candidatesOf(m, route, params, budget) } }
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
                            host.call(m.id, "custom/home", buildJsonObject { put("sourceId", src.id) })
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
                                    buildJsonObject {
                                        put("sourceId", src.id)
                                        put("query", q)
                                        put("page", 1)
                                    },
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
