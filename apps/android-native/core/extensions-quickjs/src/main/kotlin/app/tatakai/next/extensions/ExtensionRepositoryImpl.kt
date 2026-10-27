// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.io.File
import java.io.InputStream

/**
 * Installs `.kai` packages into internal storage (`<root>/<id>/{manifest.json,bundle.js,state.json}`) and resolves streams through
 * [ExtensionHost]. [openUri] turns the SAF `content://` URI (or a file path) into a stream; it is injected so the class is testable
 * without Android. [rootDir] must be an app-private directory (the Koin module passes `filesDir/extensions`).
 */
class ExtensionRepositoryImpl(
    private val rootDir: File,
    private val openUri: (String) -> InputStream,
    private val host: ExtensionHost,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : ExtensionRepository {
    private val scope = CoroutineScope(SupervisorJob() + io)
    private val state = MutableStateFlow<List<InstalledExtension>>(emptyList())
    private val loaded = HashMap<String, LoadedExtension>()
    private val lock = Mutex()
    private val ready = CompletableDeferred<Unit>()

    override val installed: StateFlow<List<InstalledExtension>> = state.asStateFlow()

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
                val bundle = File(dir, "bundle.js").readText()
                val compat = CompatAnalyzer.analyze(bundle).level
                val enabled = readEnabled(dir, default = compat != CompatLevel.UNSUPPORTED) && compat != CompatLevel.UNSUPPORTED
                register(LoadedExtension(manifest, bundle, compat, enabled))
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
            val pkg = openUri(uri).use { KaiReader.read(it) }
            id = pkg.manifest.id
            lock.withLock {
                val dir = File(rootDir, id)
                check(dir.canonicalFile.parentFile == rootDir.canonicalFile) { "bad extension directory" }
                dir.mkdirs()
                File(dir, "manifest.json").writeText(pkg.manifestText)
                File(dir, "bundle.js").writeText(pkg.bundle)
                val enabled = pkg.enabledByDefault
                writeEnabled(dir, enabled)
                register(LoadedExtension(pkg.manifest, pkg.bundle, pkg.compat.level, enabled))
                publish()
                Result.success(state.value.first { it.id == id })
            }
        } catch (e: Exception) {
            val message = when (e) {
                is ManifestException, is KaiException -> e.message.orEmpty()
                else -> "Could not read the extension package"
            }
            Result.failure(AppException(AppError.Extension(id, message), e))
        }
    }

    override suspend fun setEnabled(id: String, enabled: Boolean) {
        withContext(io) {
            ready.await()
            lock.withLock {
                val cur = loaded[id] ?: return@withContext
                val effective = enabled && cur.compat != CompatLevel.UNSUPPORTED
                writeEnabled(File(rootDir, id), effective)
                register(LoadedExtension(cur.manifest, cur.bundle, cur.compat, effective))
                publish()
            }
        }
    }

    override suspend fun remove(id: String) {
        withContext(io) {
            ready.await()
            lock.withLock {
                if (loaded.remove(id) == null) return@withContext
                host.unregister(id)
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
            loaded.values.filter { it.enabled && it.compat != CompatLevel.UNSUPPORTED && it.manifest.type != ExtensionType.TORRENT && "stream" in it.manifest.enabledAggregateRoutes }
                .sortedBy { it.manifest.priority }
        }
        if (candidates.isEmpty()) return emptyList()
        val titles = (listOf(media.title) + media.titles.values).filter { it.isNotBlank() }.distinct()
        val params = buildJsonObject {
            put("anilistId", media.ref.anilistId ?: 0)
            put("titles", JsonArray(titles.map { JsonPrimitive(it) }))
            put("episode", episode)
            media.episodes?.let { put("episodeCount", it) }
            put("stream", "0")
        }
        return coroutineScope {
            candidates.map { ext ->
                async(io) {
                    // A failing extension must not hide the others: errors become "no sources from it".
                    withTimeoutOrNull(EXTENSION_BUDGET_MS) {
                        runCatching { SourceNormalizer.toStreamSources(host.call(ext.manifest.id, "stream", params), ext.manifest.id) }
                            .getOrDefault(emptyList())
                    } ?: emptyList()
                }
            }.awaitAll().flatten()
        }
    }

    private fun register(ext: LoadedExtension) {
        loaded[ext.manifest.id] = ext
        host.register(ext)
    }

    private fun publish() {
        state.value = loaded.values.sortedBy { it.manifest.name.lowercase() }.map {
            InstalledExtension(
                id = it.manifest.id,
                name = it.manifest.name,
                version = it.manifest.version,
                type = it.manifest.type.wire,
                permissions = it.manifest.permissions,
                compat = when (it.compat) {
                    CompatLevel.FULL -> ExtensionCompat.FULL
                    CompatLevel.PARTIAL -> ExtensionCompat.PARTIAL
                    CompatLevel.UNSUPPORTED -> ExtensionCompat.UNSUPPORTED
                },
                enabled = it.enabled,
            )
        }
    }

    private fun readEnabled(dir: File, default: Boolean): Boolean =
        runCatching { Json.parseToJsonElement(File(dir, "state.json").readText()).jsonObject["enabled"]!!.jsonPrimitive.boolean }.getOrDefault(default)

    private fun writeEnabled(dir: File, enabled: Boolean) {
        dir.mkdirs()
        File(dir, "state.json").writeText(buildJsonObject { put("enabled", enabled) }.toString())
    }

    companion object {
        const val EXTENSION_BUDGET_MS = 25_000L
    }
}
