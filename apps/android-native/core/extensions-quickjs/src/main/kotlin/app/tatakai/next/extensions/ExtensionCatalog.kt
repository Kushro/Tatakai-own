// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.ExtensionRepoEntry
import app.tatakai.next.model.ExtensionUpdate
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.ProviderBookmark
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.util.Locale

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t; checklist ext.repositories, ext.updates.*,
// ext.install-url, ext.custom-providers.bookmark, ext.kill-switch, ext.preferences): the pure pieces behind ExtensionRepositoryImpl's
// wave B methods - repository index parsing, version comparison, update detection, and the small JSON stores (app-private files next to
// the installed extensions; nothing secret is written here: SECRET preferences go to SecretStore).

/** `index.json` of an extension repository: `{ name, extensions: [{ id, name, version, description, url, notes? }] }`. */
object RepoIndex {
    data class Parsed(val name: String, val entries: List<Pair<ExtensionRepoEntry, String?>>)

    /**
     * Parses [text] fetched from [indexUrl]; relative `url`s resolve against it. Also accepts a bare array and the `packages` /
     * `items` aliases. Entries without id, version or URL are skipped. @throws IllegalArgumentException when it is not an index.
     */
    fun parse(text: String, indexUrl: String): Parsed {
        val root = try {
            Json.parseToJsonElement(text)
        } catch (e: Exception) {
            throw IllegalArgumentException("not JSON", e)
        }
        val list: JsonArray
        val name: String?
        when (root) {
            is JsonArray -> {
                list = root
                name = null
            }
            is JsonObject -> {
                list = (root["extensions"] ?: root["packages"] ?: root["items"]) as? JsonArray
                    ?: throw IllegalArgumentException("no extensions[]")
                name = root.text("name")
            }
            else -> throw IllegalArgumentException("not an index")
        }
        val entries = list.mapNotNull { el ->
            val o = el as? JsonObject ?: return@mapNotNull null
            val id = o.text("id")?.takeIf { ExtensionManifest.ID_PATTERN.matches(it) } ?: return@mapNotNull null
            val version = o.text("version") ?: return@mapNotNull null
            val url = (o.text("url") ?: o.text("download") ?: o.text("package"))?.let { resolve(indexUrl, it) } ?: return@mapNotNull null
            ExtensionRepoEntry(
                id = id,
                name = o.text("name") ?: id,
                version = version,
                description = o.text("description").orEmpty(),
                url = url,
            ) to (o.text("notes") ?: o.text("changelog"))
        }.distinctBy { it.first.id }
        return Parsed(name ?: hostName(indexUrl), entries)
    }

    /** [ref] against [base] (absolute http(s) only); null when it cannot be resolved. */
    fun resolve(base: String, ref: String): String? {
        val abs = ref.toHttpUrlOrNull()
        if (abs != null) return abs.toString()
        return base.toHttpUrlOrNull()?.resolve(ref)?.toString()
    }

    fun hostName(url: String): String = url.toHttpUrlOrNull()?.host ?: url
}

/** Dotted numeric versions (`1.10.0` > `1.9.3`; `v` prefix and `-beta` suffixes tolerated: a pre-release sorts below its release). */
object Versions {
    fun compare(a: String, b: String): Int {
        fun parts(v: String): Pair<List<Int>, String?> {
            val clean = v.trim().removePrefix("v").removePrefix("V")
            val main = clean.substringBefore('-').substringBefore('+')
            val pre = clean.substringAfter('-', "").substringBefore('+').takeIf { it.isNotEmpty() }
            return main.split('.').map { p -> p.takeWhile { it.isDigit() }.toIntOrNull() ?: 0 } to pre
        }
        val (pa, prea) = parts(a)
        val (pb, preb) = parts(b)
        for (i in 0 until maxOf(pa.size, pb.size)) {
            val c = (pa.getOrElse(i) { 0 }).compareTo(pb.getOrElse(i) { 0 })
            if (c != 0) return c
        }
        return when {
            prea == null && preb == null -> 0
            prea == null -> 1
            preb == null -> -1
            else -> prea.compareTo(preb)
        }
    }

    fun isNewer(candidate: String, installed: String): Boolean = compare(candidate, installed) > 0
}

/** Updates of [installed] found in [repos] (the newest version among the repositories listing each id wins). */
fun findUpdates(installed: List<InstalledExtension>, repos: List<ExtensionRepo>, notes: Map<String, String?>): List<ExtensionUpdate> =
    installed.mapNotNull { ext ->
        val best = repos.flatMap { repo -> repo.entries.filter { it.id == ext.id }.map { repo to it } }
            .maxWithOrNull { x, y -> Versions.compare(x.second.version, y.second.version) } ?: return@mapNotNull null
        val (repo, entry) = best
        if (!Versions.isNewer(entry.version, ext.version)) return@mapNotNull null
        ExtensionUpdate(
            id = ext.id,
            name = ext.name,
            installedVersion = ext.version,
            availableVersion = entry.version,
            url = entry.url,
            notes = notes["${repo.url}|${entry.id}"],
            repositoryUrl = repo.url,
        )
    }.sortedBy { it.name.lowercase(Locale.ROOT) }

/** Marks [entries] as installed / with an update against the installed list. */
fun markInstalled(entries: List<ExtensionRepoEntry>, installed: List<InstalledExtension>): List<ExtensionRepoEntry> =
    entries.map { e ->
        val ext = installed.firstOrNull { it.id == e.id }
        e.copy(installed = ext != null, updateAvailable = ext != null && Versions.isNewer(e.version, ext.version))
    }

// ---- stores ------------------------------------------------------------------------------------------------------------------------

@Serializable
internal data class StoredRepoEntry(
    val id: String,
    val name: String,
    val version: String,
    val description: String = "",
    val url: String,
    val notes: String? = null,
)

@Serializable
internal data class StoredRepo(
    val url: String,
    val name: String,
    val entries: List<StoredRepoEntry> = emptyList(),
    val fetchedAtMs: Long? = null,
    val error: String? = null,
)

@Serializable
internal data class StoredBookmark(
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val title: String,
    val poster: String? = null,
    val completed: Boolean = false,
    val addedAtMs: Long,
)

@Serializable
internal data class CatalogState(
    val repos: List<StoredRepo> = emptyList(),
    val bookmarks: List<StoredBookmark> = emptyList(),
    /** ext.kill-switch: the ids that were enabled when "Desactivar todas" ran; null = the switch is off. */
    val killSwitchPrevious: List<String>? = null,
    /** Non-secret preference values per extension id. */
    val preferences: Map<String, Map<String, String>> = emptyMap(),
)

/** One JSON file (`<dir>/catalog.json`); writes go through a temp file + rename. Not thread-safe: the repository serialises access. */
internal class CatalogStore(private val file: File) {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    fun read(): CatalogState = runCatching { json.decodeFromString(CatalogState.serializer(), file.readText()) }.getOrDefault(CatalogState())

    fun write(state: CatalogState) {
        file.parentFile?.mkdirs()
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeText(json.encodeToString(CatalogState.serializer(), state))
        if (!tmp.renameTo(file)) {
            file.writeText(tmp.readText())
            tmp.delete()
        }
    }

    fun delete() {
        file.delete()
    }
}

internal fun StoredRepo.toModel(): ExtensionRepo = ExtensionRepo(
    url = url,
    name = name,
    entries = entries.map { ExtensionRepoEntry(it.id, it.name, it.version, it.description, it.url) },
    fetchedAtMs = fetchedAtMs,
    error = error,
)

internal fun StoredBookmark.toModel(): ProviderBookmark = ProviderBookmark(extensionId, sourceId, itemId, title, poster, completed, addedAtMs)

internal fun ProviderBookmark.toStored(): StoredBookmark = StoredBookmark(extensionId, sourceId, itemId, title, poster, completed, addedAtMs)

/** Blocking GET of [url] with a size cap (call it on an IO dispatcher). @throws IOException on HTTP errors or oversize bodies. */
internal fun httpGetBytes(client: OkHttpClient, url: String, maxBytes: Long, accept: String? = null): ByteArray {
    val parsed = url.toHttpUrlOrNull() ?: throw IOException("invalid URL")
    if (parsed.scheme != "https" && parsed.scheme != "http") throw IOException("unsupported scheme")
    val request = Request.Builder().url(parsed).apply { if (accept != null) header("Accept", accept) }.get().build()
    client.newCall(request).execute().use { response ->
        if (!response.isSuccessful) throw IOException("HTTP ${response.code}")
        val body = response.body
        if (body.contentLength() > maxBytes) throw IOException("response too large")
        val source = body.source()
        if (source.request(maxBytes + 1)) throw IOException("response too large")
        return source.buffer.readByteArray()
    }
}

private fun JsonObject.text(key: String): String? =
    (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }
