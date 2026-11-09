// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.Media
import app.tatakai.next.model.Quality
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.StremioAddon
import app.tatakai.next.model.StremioAddonRepository
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
import app.tatakai.next.release.parseRelease
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
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import java.io.File
import java.net.URLEncoder
import java.text.Normalizer
import java.util.Locale

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t, checklist ext.stremio / ext.stremio.manage, plan §7
// D4): native Stremio addons. Port of the classic src/core/extensions/remote/{stremio,stremio-search}.ts (same user, AGPL).
//
// `install` fetches `<manifestUrl>` (`stremio://host/manifest.json` -> https), validates `id`, `name`, `version` and that `resources`
// has `stream`, stores the addon in [storeDir]/addons.json. Streams: `GET <base>/stream/<type>/<id>.json` with `<id>` =
// `kitsu:<kitsuId>:<episode>` / `mal:<malId>:<episode>` when the addon lists those `idPrefixes`, else the IMDb episode id found through
// Cinemeta (`v3-cinemeta.strem.io`: exact title match + `meta.videos`); the title -> id match is cached per addon in
// [storeDir]/matches/<addonId>.json (`resetMatches` deletes it). Each stream becomes a web candidate (`url`, headers from
// `behaviorHints.proxyHeaders.request`) or a torrent one (`infoHash` + `fileIdx`), `extId = "stremio:<id>"`, `extName = addon.name`.

/** Parsed `manifest.json` of an addon (only what the app needs). */
data class StremioManifest(
    val id: String,
    val name: String,
    val version: String,
    val description: String,
    val logo: String?,
    val types: List<String>,
    /** Resources by name -> their own `types` / `idPrefixes` (null = the manifest-level ones). */
    val streamTypes: List<String>?,
    val idPrefixes: List<String>,
    val hasStreams: Boolean,
)

object StremioWire {
    private val ID = Regex("^[A-Za-z0-9._:-]{1,128}$")

    /** `stremio://x/manifest.json` -> `https://x/manifest.json`; anything that is not http(s) after that -> null. */
    fun normalizeManifestUrl(raw: String): String? {
        val t = raw.trim()
        val https = if (t.startsWith("stremio://", ignoreCase = true)) "https://" + t.substring("stremio://".length) else t
        val url = https.toHttpUrlOrNull() ?: return null
        return url.toString()
    }

    /** `https://x/y/manifest.json` -> `https://x/y` (the base every resource path hangs from). */
    fun baseUrl(manifestUrl: String): String = manifestUrl.substringBefore('?').removeSuffix("/manifest.json").trimEnd('/')

    /** `<base>/<resource>/<type>/<id>.json` with the id percent-encoded like the Stremio SDK. */
    fun resourceUrl(manifestUrl: String, resource: String, type: String, id: String): String =
        "${baseUrl(manifestUrl)}/$resource/${enc(type)}/${enc(id)}.json"

    private fun enc(s: String): String = URLEncoder.encode(s, "UTF-8").replace("+", "%20").replace("%3A", ":")

    /** @throws IllegalArgumentException with the failing field. */
    fun parseManifest(text: String): StremioManifest {
        val o = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: throw IllegalArgumentException("manifest is not a JSON object")
        val id = o.text("id")?.takeIf { ID.matches(it) } ?: throw IllegalArgumentException("manifest.id")
        val name = o.text("name") ?: throw IllegalArgumentException("manifest.name")
        val version = o.text("version") ?: throw IllegalArgumentException("manifest.version")
        val types = strings(o["types"])
        val resources = o["resources"] as? JsonArray ?: throw IllegalArgumentException("manifest.resources")
        var streamTypes: List<String>? = null
        var hasStreams = false
        var prefixes = strings(o["idPrefixes"])
        for (r in resources) {
            when (r) {
                is JsonPrimitive -> if (r.contentOrNull == "stream") hasStreams = true
                is JsonObject -> if (r.text("name") == "stream") {
                    hasStreams = true
                    strings(r["types"]).takeIf { it.isNotEmpty() }?.let { streamTypes = it }
                    strings(r["idPrefixes"]).takeIf { it.isNotEmpty() }?.let { prefixes = it }
                }
                else -> Unit
            }
        }
        return StremioManifest(id, name, version, o.text("description").orEmpty(), o.text("logo"), types, streamTypes, prefixes, hasStreams)
    }

    /** Stream ids the addon can answer for [media] / [episode] without a title search, best first: (type, id). */
    fun directTargets(manifest: StremioAddon, prefixes: List<String>, media: Media, episode: Int): List<Pair<String, String>> {
        val type = seriesType(manifest.types)
        val out = ArrayList<Pair<String, String>>(2)
        if (prefixes.any { it.startsWith("kitsu") }) media.ref.kitsuId?.let { out += type to "kitsu:$it:$episode" }
        if (prefixes.any { it.startsWith("mal") }) media.ref.malId?.let { out += type to "mal:$it:$episode" }
        if (prefixes.any { it.startsWith("anilist") }) media.ref.anilistId?.let { out += type to "anilist:$it:$episode" }
        return out
    }

    /** `anime` when the addon lists it, else `series`. */
    fun seriesType(types: List<String>): String = if ("anime" in types) "anime" else "series"

    /** Whether the addon takes IMDb ids (no `idPrefixes` = everything, or `tt`). */
    fun acceptsImdb(prefixes: List<String>): Boolean = prefixes.isEmpty() || prefixes.any { it.startsWith("tt") }

    /**
     * Cinemeta `meta.videos` -> the video id of absolute [episode] (classic `episodeVideoId`): videos without seasons match by number;
     * with seasons, the regular seasons (> 0) are walked in order and must be contiguous, then the n-th video wins.
     */
    fun episodeVideoId(meta: JsonObject, episode: Int): String? {
        if (meta.text("type") == "movie") return meta.text("id")
        val videos = (meta["videos"] as? JsonArray).orEmpty().mapNotNull { v ->
            val o = v as? JsonObject ?: return@mapNotNull null
            val id = o.text("id") ?: return@mapNotNull null
            val ep = (o["episode"] as? JsonPrimitive)?.intOrNull ?: (o["number"] as? JsonPrimitive)?.intOrNull ?: return@mapNotNull null
            Triple(id, (o["season"] as? JsonPrimitive)?.takeUnless { it is JsonNull }?.intOrNull, ep)
        }
        if (videos.all { it.second == null }) return videos.filter { it.third == episode }.singleOrNull()?.first
        val regular = videos.filter { (it.second ?: 0) > 0 }.sortedWith(compareBy({ it.second }, { it.third }))
        var season = 0
        var number = 0
        for (v in regular) {
            if (v.second != season) {
                if (v.second != season + 1) return null
                season = v.second ?: return null
                number = 0
            }
            number++
            if (v.third != number) return null
        }
        return regular.getOrNull(episode - 1)?.first
    }

    /** Lower-case letters and digits only (classic `normalize`): "Sousou no Frieren!" == "sousou no frieren". */
    fun normalizeTitle(s: String): String =
        Normalizer.normalize(s, Normalizer.Form.NFKD).lowercase(Locale.ROOT).filter { it.isLetterOrDigit() }

    /** The single Cinemeta meta whose name matches one of [titles] exactly (normalised); null when none or ambiguous. */
    fun pickMeta(metas: JsonArray, titles: List<String>): JsonObject? {
        val wanted = titles.map(::normalizeTitle).filter { it.isNotEmpty() }.toSet()
        val matches = metas.mapNotNull { it as? JsonObject }.filter { m ->
            val n = m.text("name") ?: m.text("title") ?: return@filter false
            normalizeTitle(n) in wanted
        }.distinctBy { it.text("id") }
        return matches.singleOrNull()
    }

    /** `{ streams: [...] }` -> candidates of [addon] in answer order; labels repeated get `#n`. */
    fun streamsToCandidates(addon: StremioAddon, body: JsonElement?): List<SourceCandidate> {
        val streams = ((body as? JsonObject)?.get("streams") as? JsonArray).orEmpty()
        val extId = StremioCandidateSource.EXT_ID_PREFIX + addon.id
        val out = ArrayList<SourceCandidate>()
        for (el in streams) {
            val s = el as? JsonObject ?: continue
            val nameLine = (s.text("name") ?: s.text("title") ?: addon.name).lineSequence().first().trim()
            val titleText = s.text("title") ?: s.text("description") ?: ""
            val quality = Quality.parse(listOf(nameLine, titleText).firstNotNullOfOrNull { q -> QUALITY.find(q)?.value })
            val hints = s["behaviorHints"] as? JsonObject
            val headers = ((hints?.get("proxyHeaders") as? JsonObject)?.get("request") as? JsonObject)
                ?.mapNotNull { (k, v) -> (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.let { k to it } }?.toMap().orEmpty()
            var label = nameLine.ifEmpty { addon.name }
            if (out.any { it.label == label }) {
                var n = 2
                while (out.any { it.label == "$label #$n" }) n++
                label = "$label #$n"
            }
            val url = s.text("url")
            val infoHash = s.text("infoHash")?.let { Candidates.hexInfoHash(it) }
            when {
                url != null && (url.startsWith("https://") || url.startsWith("http://")) -> {
                    val path = url.substringBefore('?').lowercase(Locale.ROOT)
                    val container = when {
                        path.endsWith(".m3u8") -> "HLS"
                        path.endsWith(".mpd") -> continue
                        else -> "MP4"
                    }
                    out += SourceCandidate(
                        extId = extId, extName = addon.name, kind = SourceKind.WEB, label = label, quality = quality,
                        container = container, reliability = INITIAL_RELIABILITY,
                        note = titleText.lineSequence().firstOrNull()?.trim()?.takeIf { it.isNotEmpty() && it != nameLine },
                        web = WebInfo(url = url, headers = headers),
                    )
                }
                infoHash != null -> {
                    val release = titleText.lineSequence().firstOrNull()?.trim().orEmpty()
                    var tags = if (release.isNotEmpty()) parseRelease(release) else ReleaseTags()
                    if (tags.resolution == Quality.UNKNOWN) tags = tags.copy(resolution = quality)
                    val fileIdx = (s["fileIdx"] as? JsonPrimitive)?.intOrNull
                    val magnet = "magnet:?xt=urn:btih:$infoHash" + (if (release.isNotEmpty()) "&dn=" + URLEncoder.encode(release, "UTF-8") else "")
                    out += SourceCandidate(
                        extId = extId, extName = addon.name, kind = SourceKind.TORRENT, label = tags.group ?: label,
                        quality = if (quality == Quality.UNKNOWN) tags.resolution else quality,
                        reliability = INITIAL_RELIABILITY,
                        note = fileIdx?.let { "file $it" },
                        torrent = TorrentInfo(infoHash = infoHash, seeders = SEEDERS.find(titleText)?.groupValues?.get(1)?.toIntOrNull() ?: 0, magnet = magnet),
                        tags = tags,
                    )
                }
            }
        }
        return out
    }

    private val QUALITY = Regex("(?i)\\b(2160p|4k|1440p|1080p|720p|480p)\\b")
    private val SEEDERS = Regex("👤\\s*(\\d+)")

    private fun strings(el: JsonElement?): List<String> =
        (el as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content?.trim()?.takeIf { s -> s.isNotEmpty() } }

    private fun JsonObject.text(key: String): String? =
        (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }
}

class StremioAddonRepositoryImpl(
    private val client: OkHttpClient,
    private val storeDir: File,
    private val logs: ExtensionLogStore? = null,
    private val clock: () -> Long = System::currentTimeMillis,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) : StremioAddonRepository {
    private val scope = CoroutineScope(SupervisorJob() + io)
    private val lock = Mutex()
    private val ready = CompletableDeferred<Unit>()
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }
    private val _addons = MutableStateFlow<List<StremioAddon>>(emptyList())
    override val addons: StateFlow<List<StremioAddon>> = _addons.asStateFlow()

    /** addon id -> its parsed manifest (stream types / id prefixes); filled at install / update / load. */
    private val manifests = HashMap<String, StremioManifest>()

    init {
        scope.launch {
            lock.withLock {
                val stored = runCatching { json.decodeFromString(ListSerializer(StremioAddon.serializer()), addonsFile().readText()) }.getOrDefault(emptyList())
                _addons.value = stored
                stored.forEach { a ->
                    runCatching { StremioWire.parseManifest(manifestFile(a.id).readText()) }.getOrNull()?.let { manifests[a.id] = it }
                }
            }
            ready.complete(Unit)
        }
    }

    override suspend fun install(manifestUrl: String): Result<StremioAddon> = withContext(io) {
        ready.await()
        val url = StremioWire.normalizeManifestUrl(manifestUrl)
            ?: return@withContext Result.failure(IllegalArgumentException("Not an http(s) or stremio:// manifest URL"))
        try {
            val (manifest, text) = fetchManifest(url)
            if (!manifest.hasStreams) throw IllegalArgumentException("This addon has no stream resource")
            val addon = lock.withLock {
                val previous = _addons.value.firstOrNull { it.id == manifest.id }
                val addon = StremioAddon(
                    id = manifest.id, name = manifest.name, version = manifest.version, description = manifest.description,
                    manifestUrl = url, logo = manifest.logo, enabled = previous?.enabled ?: true, types = manifest.types,
                    hasStreams = manifest.hasStreams, installedAtMs = previous?.installedAtMs ?: clock(),
                    updatedAtMs = if (previous != null) clock() else null,
                )
                manifests[addon.id] = manifest
                manifestFile(addon.id).parentFile?.mkdirs()
                manifestFile(addon.id).writeText(text)
                save(_addons.value.filterNot { it.id == addon.id } + addon)
                addon
            }
            logs?.ok(extIdOf(addon.id), "install", "installed Stremio addon ${addon.name} v${addon.version}")
            Result.success(addon)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(e)
        }
    }

    override suspend fun setEnabled(id: String, enabled: Boolean) {
        withContext(io) {
            ready.await()
            lock.withLock { save(_addons.value.map { if (it.id == id) it.copy(enabled = enabled) else it }) }
        }
    }

    override suspend fun remove(id: String) {
        withContext(io) {
            ready.await()
            lock.withLock {
                save(_addons.value.filterNot { it.id == id })
                manifests.remove(id)
                manifestFile(id).delete()
                matchesFile(id).delete()
            }
            logs?.clear(extIdOf(id))
        }
    }

    override suspend fun update(id: String): Result<StremioAddon> {
        ready.await()
        val current = _addons.value.firstOrNull { it.id == id } ?: return Result.failure(NoSuchElementException("Unknown addon"))
        return install(current.manifestUrl)
    }

    override suspend fun resetMatches(id: String) {
        withContext(io) {
            lock.withLock { matchesFile(id).delete() }
            logs?.ok(extIdOf(id), "stremio", "title matches reset")
        }
    }

    /** "Restablecer la app": forgets every addon and cached match. */
    suspend fun wipe() {
        withContext(io) {
            lock.withLock {
                _addons.value = emptyList()
                manifests.clear()
                storeDir.deleteRecursively()
            }
        }
    }

    // ---- used by StremioCandidateSource ------------------------------------------------------------------------------------------

    internal suspend fun manifestOf(id: String): StremioManifest? {
        ready.await()
        return lock.withLock { manifests[id] }
    }

    internal suspend fun cachedMatch(addonId: String, key: String): String? = withContext(io) {
        lock.withLock { readMatches(addonId)[key] }
    }

    internal suspend fun storeMatch(addonId: String, key: String, value: String) {
        withContext(io) {
            lock.withLock {
                val map = readMatches(addonId).toMutableMap()
                map[key] = value
                while (map.size > MAX_MATCHES) map.remove(map.keys.first())
                matchesFile(addonId).parentFile?.mkdirs()
                matchesFile(addonId).writeText(json.encodeToString(MapSerializer(String.serializer(), String.serializer()), map))
            }
        }
    }

    internal fun log(addonId: String, ok: Boolean, message: String) {
        if (ok) logs?.ok(extIdOf(addonId), "stremio", message) else logs?.warn(extIdOf(addonId), "stremio", message)
    }

    private fun readMatches(addonId: String): Map<String, String> =
        runCatching { json.decodeFromString(MapSerializer(String.serializer(), String.serializer()), matchesFile(addonId).readText()) }
            .getOrDefault(emptyMap())

    private fun fetchManifest(url: String): Pair<StremioManifest, String> {
        val text = httpGetBytes(client, url, MAX_MANIFEST_BYTES, "application/json").decodeToString()
        return StremioWire.parseManifest(text) to text
    }

    private fun save(list: List<StremioAddon>) {
        storeDir.mkdirs()
        addonsFile().writeText(json.encodeToString(ListSerializer(StremioAddon.serializer()), list))
        _addons.value = list.sortedBy { it.name.lowercase(Locale.ROOT) }
    }

    private fun addonsFile() = File(storeDir, "addons.json")

    private fun manifestFile(id: String) = File(storeDir, "manifests/${safe(id)}.json")

    private fun matchesFile(id: String) = File(storeDir, "matches/${safe(id)}.json")

    private fun safe(id: String) = id.replace(Regex("[^A-Za-z0-9._-]"), "_")

    companion object {
        const val DIR_NAME: String = "stremio"
        const val MAX_MANIFEST_BYTES = 512L * 1024
        private const val MAX_MATCHES = 500

        fun extIdOf(addonId: String) = StremioCandidateSource.EXT_ID_PREFIX + addonId
    }
}

/** The enabled Stremio addons as one more provider group of `ExtensionRepositoryImpl.resolveCandidates` (like the PC servers). */
class StremioCandidateSource(
    private val repository: StremioAddonRepositoryImpl,
    private val client: OkHttpClient,
    private val budgetMs: Long = ADDON_BUDGET_MS,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val cinemetaBase: String = CINEMETA,
) : ExternalCandidateSource {
    override val available: Flow<Boolean> = repository.addons.map { list -> list.any { it.enabled && it.hasStreams } }

    override suspend fun candidates(media: Media, episode: Int): List<SourceCandidate> {
        val enabled = repository.addons.value.filter { it.enabled && it.hasStreams }
        if (enabled.isEmpty()) return emptyList()
        return coroutineScope {
            enabled.map { addon ->
                async(io) {
                    withTimeoutOrNull(budgetMs) {
                        try {
                            addonCandidates(addon, media, episode)
                        } catch (e: CancellationException) {
                            throw e
                        } catch (e: Exception) {
                            repository.log(addon.id, false, e.message ?: "stream lookup failed")
                            emptyList()
                        }
                    } ?: emptyList<SourceCandidate>().also { repository.log(addon.id, false, "no answer within ${budgetMs / 1000} s") }
                }
            }.awaitAll().flatten()
        }
    }

    private suspend fun addonCandidates(addon: StremioAddon, media: Media, episode: Int): List<SourceCandidate> {
        val manifest = repository.manifestOf(addon.id)
        val prefixes = manifest?.idPrefixes.orEmpty()
        val targets = StremioWire.directTargets(addon, prefixes, media, episode).toMutableList()
        if (targets.isEmpty() && StremioWire.acceptsImdb(prefixes)) imdbTarget(addon, media, episode)?.let { targets += it }
        if (targets.isEmpty()) {
            repository.log(addon.id, false, "no id this addon accepts for the title")
            return emptyList()
        }
        for ((type, id) in targets) {
            val body = withContext(io) {
                Json.parseToJsonElement(httpGetBytes(client, StremioWire.resourceUrl(addon.manifestUrl, "stream", type, id), MAX_STREAM_BYTES).decodeToString())
            }
            val list = StremioWire.streamsToCandidates(addon, body)
            if (list.isNotEmpty()) {
                repository.log(addon.id, true, "${list.size} streams")
                return list
            }
        }
        repository.log(addon.id, false, "no streams for this episode")
        return emptyList()
    }

    /** IMDb episode id through Cinemeta (exact title, cached per addon). */
    private suspend fun imdbTarget(addon: StremioAddon, media: Media, episode: Int): Pair<String, String>? {
        val titles = (listOf(media.title) + media.titles.values).filter { it.isNotBlank() }.distinct()
        if (titles.isEmpty()) return null
        val key = (media.ref.anilistId?.let { "al:$it" } ?: StremioWire.normalizeTitle(titles.first())) + ":$episode"
        repository.cachedMatch(addon.id, key)?.let { return "series" to it }
        for (title in titles.take(3)) {
            val search = "$cinemetaBase/catalog/series/top/search=${URLEncoder.encode(title, "UTF-8").replace("+", "%20")}.json"
            val metas = runCatching {
                withContext(io) { (Json.parseToJsonElement(httpGetBytes(client, search, MAX_STREAM_BYTES).decodeToString()) as? JsonObject)?.get("metas") as? JsonArray }
            }.getOrNull() ?: continue
            val pick = StremioWire.pickMeta(metas, titles) ?: continue
            val imdb = (pick["id"] as? JsonPrimitive)?.contentOrNull ?: continue
            val meta = runCatching {
                withContext(io) {
                    (Json.parseToJsonElement(httpGetBytes(client, "$cinemetaBase/meta/series/$imdb.json", MAX_STREAM_BYTES).decodeToString()) as? JsonObject)
                        ?.get("meta") as? JsonObject
                }
            }.getOrNull() ?: continue
            val videoId = StremioWire.episodeVideoId(meta, episode) ?: continue
            repository.storeMatch(addon.id, key, videoId)
            return "series" to videoId
        }
        return null
    }

    companion object {
        const val ADDON_BUDGET_MS: Long = 15_000
        const val EXT_ID_PREFIX: String = "stremio:"
        const val CINEMETA: String = "https://v3-cinemeta.strem.io"
        private const val MAX_STREAM_BYTES = 4L * 1024 * 1024
    }
}
