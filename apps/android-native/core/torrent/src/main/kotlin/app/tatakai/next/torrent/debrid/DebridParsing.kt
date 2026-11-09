// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.debrid

import app.tatakai.next.model.torrent.DebridService
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import java.time.Instant
import java.util.Locale

// Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, worker q; checklist sources.debrid.*). Pure parsing of the
// debrid APIs, ported from the classic clients (src/core/providers/realdebrid-client.ts, torbox-client.ts, cloud-debrid-client.ts,
// debrid-orchestrator.ts). Unit tested with recorded answers.

internal val debridJson = Json { ignoreUnknownKeys = true; isLenient = true }

/** What the account endpoint says. */
internal data class DebridUser(val username: String?, val premiumUntilMs: Long?)

/** A file of a torrent as a debrid service lists it. [id] = service id (RD file id, TorBox file id, AllDebrid link, Premiumize link). */
internal data class RemoteFile(val id: String, val name: String, val size: Long)

/** Thrown when a service answered but the torrent is not instantly available. */
internal class NotCachedException(service: DebridService) : Exception("debrid:not_cached:${service.id}")

internal fun parseObject(text: String): JsonObject? = runCatching { debridJson.parseToJsonElement(text) as? JsonObject }.getOrNull()

internal fun JsonElement?.str(): String? = (this as? JsonPrimitive)?.takeIf { this !is JsonNull }?.contentOrNull

internal fun JsonElement?.long(): Long? = str()?.toDoubleOrNull()?.toLong()

internal fun JsonElement?.bool(): Boolean? = (this as? JsonPrimitive)?.booleanOrNull

internal fun JsonElement?.obj(): JsonObject? = this as? JsonObject

internal fun JsonElement?.arr(): JsonArray? = this as? JsonArray

/** Unix seconds or ISO-8601 -> epoch ms; null when absent / zero. */
internal fun parseInstantMs(value: JsonElement?): Long? {
    val raw = value.str()?.trim()?.takeIf { it.isNotEmpty() && it != "0" } ?: return null
    raw.toLongOrNull()?.let { return if (it > 100_000_000_000L) it else it * 1000 }
    return runCatching { Instant.parse(raw).toEpochMilli() }.getOrNull()
        ?: runCatching { Instant.parse(raw.replace(' ', 'T') + if (raw.endsWith("Z")) "" else "Z").toEpochMilli() }.getOrNull()
}

/** `/user` (RD), `/api/user/me` (TorBox), `/v4/user` (AllDebrid), `/api/account/info` (Premiumize). Null = not a valid account answer. */
internal fun parseUser(service: DebridService, text: String): DebridUser? {
    val o = parseObject(text) ?: return null
    return when (service) {
        DebridService.REAL_DEBRID -> {
            val name = o["username"].str() ?: return null
            DebridUser(name, parseInstantMs(o["expiration"]))
        }
        DebridService.TORBOX -> {
            if (o["success"].bool() == false) return null
            val data = o["data"].obj() ?: return null
            DebridUser(data["email"].str() ?: data["customer"].str(), parseInstantMs(data["premium_expires_at"]))
        }
        DebridService.ALLDEBRID -> {
            if (o["status"].str() != "success") return null
            val user = o["data"].obj()?.get("user").obj() ?: return null
            DebridUser(user["username"].str(), parseInstantMs(user["premiumUntil"]))
        }
        DebridService.PREMIUMIZE -> {
            if (o["status"].str() != "success") return null
            DebridUser(o["customer_id"].str(), parseInstantMs(o["premium_until"]))
        }
    }
}

/** TorBox `checkcached?format=object`: the `data` object is keyed by the cached hashes (null / empty when none). */
internal fun parseTorboxCached(text: String, hashes: List<String>): Set<String> {
    val o = parseObject(text) ?: return emptySet()
    val keys = o["data"].obj()?.keys?.map { it.lowercase(Locale.ROOT) }?.toSet().orEmpty()
    return hashes.filter { it.lowercase(Locale.ROOT) in keys }.toSet()
}

/** Premiumize `cache/check`: `response` is a boolean per requested item, same order. */
internal fun parsePremiumizeCached(text: String, hashes: List<String>): Set<String> {
    val o = parseObject(text) ?: return emptySet()
    if (o["status"].str() != "success") return emptySet()
    val answers = o["response"].arr() ?: return emptySet()
    return hashes.filterIndexed { i, _ -> answers.getOrNull(i).bool() == true }.toSet()
}

/** Real-Debrid `/torrents/info/{id}` -> status, files, links. */
internal data class RdInfo(val status: String, val files: List<RemoteFile>, val links: List<String>)

internal fun parseRdInfo(text: String): RdInfo? {
    val o = parseObject(text) ?: return null
    val files = o["files"].arr()?.mapNotNull { f ->
        val fo = f.obj() ?: return@mapNotNull null
        val id = fo["id"].str() ?: return@mapNotNull null
        RemoteFile(id, fo["path"].str().orEmpty().trimStart('/'), fo["bytes"].long() ?: 0)
    }.orEmpty()
    val links = o["links"].arr()?.mapNotNull { it.str() }.orEmpty()
    return RdInfo(o["status"].str().orEmpty().lowercase(Locale.ROOT), files, links)
}

internal val RD_FAILED = setOf("magnet_error", "error", "virus", "dead")

/** Torrent statuses that mean "RD is fetching it": not instantly available. */
internal val RD_NOT_CACHED = setOf("queued", "downloading", "compressing", "uploading")

/** TorBox `mylist` entry (array or single object) matching [id]. */
internal data class TorboxInfo(val present: Boolean, val state: String, val files: List<RemoteFile>, val error: String?)

internal fun parseTorboxInfo(text: String, id: String): TorboxInfo? {
    val o = parseObject(text) ?: return null
    val data = o["data"]
    val item = when (data) {
        is JsonArray -> data.mapNotNull { it.obj() }.firstOrNull { it["id"].str() == id }
        is JsonObject -> data.takeIf { it["id"].str() == id }
        else -> null
    } ?: return null
    return TorboxInfo(
        present = item["download_present"].bool() == true,
        state = item["download_state"].str().orEmpty().lowercase(Locale.ROOT),
        files = item["files"].arr()?.mapNotNull { f ->
            val fo = f.obj() ?: return@mapNotNull null
            val fid = fo["id"].str() ?: return@mapNotNull null
            RemoteFile(fid, fo["short_name"].str() ?: fo["name"].str() ?: fo["absolute_path"].str().orEmpty(), fo["size"].long() ?: 0)
        }.orEmpty(),
        error = item["error"].str(),
    )
}

/** TorBox `mylist` entries that already hold [hash] (dedupe instead of creating a duplicate). */
internal fun torboxExistingId(text: String, hash: String): String? {
    val o = parseObject(text) ?: return null
    val items = when (val d = o["data"]) {
        is JsonArray -> d.mapNotNull { it.obj() }
        is JsonObject -> listOf(d)
        else -> emptyList()
    }
    return items.firstOrNull { it["hash"].str()?.lowercase(Locale.ROOT) == hash.lowercase(Locale.ROOT) && it["download_present"].bool() == true }
        ?.get("id").str()
}

/** AllDebrid `magnet/files`: nested `{n, s, l}` / `{n, e: [...]}` tree flattened to files (id = the link to unlock). */
internal fun parseAllDebridFiles(text: String, magnetId: String): List<RemoteFile> {
    val o = parseObject(text) ?: return emptyList()
    val magnets = o["data"].obj()?.get("magnets").arr() ?: return emptyList()
    val entry = magnets.mapNotNull { it.obj() }.firstOrNull { it["id"].str() == magnetId } ?: magnets.firstOrNull().obj() ?: return emptyList()
    val out = ArrayList<RemoteFile>()
    fun walk(nodes: JsonArray?, prefix: String, depth: Int) {
        if (nodes == null || depth > 12 || out.size > 1000) return
        for (n in nodes) {
            val node = n.obj() ?: continue
            val name = node["n"].str().orEmpty()
            val children = node["e"].arr()
            val link = node["l"].str()
            if (children != null) walk(children, "$prefix$name/", depth + 1) else if (link != null) out += RemoteFile(link, "$prefix$name", node["s"].long() ?: 0)
        }
    }
    walk(entry["files"].arr(), "", 0)
    return out
}

/** AllDebrid `magnet/status` statusCode (4 = ready, > 4 = error) of [magnetId]; null when absent. */
internal fun parseAllDebridStatus(text: String, magnetId: String): Int? {
    val o = parseObject(text) ?: return null
    val m = o["data"].obj()?.get("magnets") ?: return null
    val entry = when (m) {
        is JsonArray -> m.mapNotNull { it.obj() }.firstOrNull { it["id"].str() == magnetId }
        is JsonObject -> m
        else -> null
    } ?: return null
    return entry["statusCode"].long()?.toInt()
}

/** Premiumize `transfer/directdl`: `content[{path, size, link}]`. */
internal fun parsePremiumizeContent(text: String): List<RemoteFile> {
    val o = parseObject(text) ?: return emptyList()
    if (o["status"].str() != "success") return emptyList()
    return o["content"].arr()?.mapNotNull { f ->
        val fo = f.obj() ?: return@mapNotNull null
        val link = fo["link"].str()?.takeIf { it.startsWith("http") } ?: return@mapNotNull null
        RemoteFile(link, fo["path"].str().orEmpty(), fo["size"].long() ?: 0)
    }.orEmpty()
}

private val VIDEO_EXTENSION = Regex("""\.(?:mp4|mkv|webm|m4v|mov|avi|ts|m2ts|mts|mpg|mpeg|m2v|wmv|flv|ogv|3gp)(?:$|[?#])""", RegexOption.IGNORE_CASE)

internal fun isDebridVideo(name: String): Boolean = VIDEO_EXTENSION.containsMatchIn(name)

/** `s01e05`, `ep 05`, ` 05 ` (with optional `v2`), as the classic `matchesEpisode`. */
internal fun matchesEpisode(name: String, episode: Int?): Boolean {
    val n = episode?.takeIf { it > 0 } ?: return false
    val alt = "(?:%02d|%d)".format(n, n)
    return listOf(
        Regex("""(?:^|[^a-z0-9])s\d{1,3}e$alt(?:v\d+)?(?:[^0-9]|$)""", RegexOption.IGNORE_CASE),
        Regex("""(?:^|[^a-z0-9])ep(?:isode)?[ ._-]*$alt(?:v\d+)?(?:[^0-9]|$)""", RegexOption.IGNORE_CASE),
        Regex("""(?:^|[ ._\-])$alt(?:v\d+)?(?:[ ._\-]|$)""", RegexOption.IGNORE_CASE),
    ).any { it.containsMatchIn(name) }
}

/** The intended episode when possible, otherwise the largest video (classic `selectBestDebridVideoFile`). */
internal fun selectBestVideo(files: List<RemoteFile>, episode: Int?): RemoteFile? {
    val videos = files.filter { isDebridVideo(it.name) }
    if (videos.isEmpty()) return null
    return videos.sortedWith(
        compareByDescending<RemoteFile> { matchesEpisode(it.name.substringAfterLast('/'), episode) }.thenByDescending { it.size },
    ).first()
}

/** 40-hex or 32-char base32 btih of a magnet -> lowercase hex. */
internal fun magnetHash(magnet: String): String? {
    val m = Regex("""btih:([a-f0-9]{40}|[a-z2-7]{32})""", RegexOption.IGNORE_CASE).find(magnet) ?: return null
    val raw = m.groupValues[1]
    if (raw.length == 40) return raw.lowercase(Locale.ROOT)
    val alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
    var buffer = 0
    var bits = 0
    val sb = StringBuilder()
    for (c in raw.uppercase(Locale.ROOT)) {
        buffer = (buffer shl 5) or alphabet.indexOf(c)
        bits += 5
        if (bits >= 8) {
            bits -= 8
            sb.append("%02x".format((buffer shr bits) and 0xff))
        }
    }
    return sb.toString()
}
