// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensions.Candidates
import app.tatakai.next.model.SourceCandidate
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull

/**
 * `/resolve` snapshot -> [SourceCandidate]s (docs/redesign/13 §3.10). One provider group per server provider (`providerKey`):
 * `extId = "server:<serverId>/<provider row id>"`, `extName = providerName` (the server already sends `"<source> · <server name>"`).
 * The entries go through the same [Candidates.mapCandidates] as the installed extensions, so the existing ranking orders them.
 *
 * Safety: a web entry is kept only when its URL is on the server's own origin (the PC's media proxy under `/api/server/v1/media/`) and passes
 * [HostGuard]; `embed` / `custom` pages (not proxied by the server) are dropped by the mapper. Subtitle tracks follow the same rule.
 * Torrent entries (magnet / info-hash) are kept: they are played by this phone's torrent engine.
 */
object ServerCandidates {
    const val EXT_PREFIX = "server:"

    /** Whether [extId] belongs to an extension server group. */
    fun isServerExtId(extId: String): Boolean = extId.startsWith(EXT_PREFIX)

    fun map(conn: ExtensionServerConnection, snapshot: ResolveSnapshot): List<SourceCandidate> {
        val groups = LinkedHashMap<String, MutableList<JsonObject>>()
        val names = HashMap<String, String>()
        for (source in snapshot.sourceObjects) {
            val provider = text(source["providerKey"]) ?: text(source["extensionId"]) ?: "default"
            val prepared = prepare(conn, source) ?: continue
            groups.getOrPut(provider) { ArrayList() } += prepared
            if (provider !in names) names[provider] = extNameOf(conn, source)
        }
        return groups.flatMap { (provider, list) ->
            Candidates.mapCandidates(extIdOf(conn, provider), names.getValue(provider), JsonObject(mapOf("sources" to JsonArray(list))))
        }
    }

    /** `server:<serverId>/<row>`: the server's providerKey is `<serverId>/<row>`, the prefix is not repeated. No `|` (sourceKey). */
    fun extIdOf(conn: ExtensionServerConnection, providerKey: String): String {
        val row = providerKey.removePrefix("${conn.serverId}/").ifEmpty { "default" }
        return (EXT_PREFIX + conn.serverId + "/" + row).replace('|', '_')
    }

    private fun extNameOf(conn: ExtensionServerConnection, s: JsonObject): String =
        text(s["providerName"])
            ?: text(s["extensionName"])?.let { "$it · ${conn.name}" }
            ?: conn.name

    /**
     * Normalizes one server entry for [Candidates.mapCandidates]: label from `sourceName` (the server's per-entry name) when there is no
     * `server`, `type: hls` for `isM3U8` entries without a type, no headers (the PC's proxy already carries them), and only same-origin
     * URLs. Null = drop the entry.
     */
    internal fun prepare(conn: ExtensionServerConnection, s: JsonObject): JsonObject? {
        val out = LinkedHashMap(s)
        val type = text(s["type"]) ?: text(s["sourceType"])
        val torrent = type == "torrent" || (s["isTorrent"] as? JsonPrimitive)?.booleanOrNull == true
        if (!torrent) {
            val url = text(s["url"]) ?: return null
            if (!HostGuard.isAllowed(url) || !HostGuard.sameOrigin(url, conn.url)) return null
            if (type == null && (s["isM3U8"] as? JsonPrimitive)?.booleanOrNull == true) out["type"] = JsonPrimitive("hls")
        }
        if (text(s["server"]) == null) text(s["sourceName"])?.let { out["server"] = JsonPrimitive(it) }
        out.remove("headers")
        (s["subtitles"] as? JsonArray)?.let { subs ->
            out["subtitles"] = JsonArray(
                subs.filter { t ->
                    val u = ((t as? JsonObject)?.get("url") as? JsonPrimitive)?.contentOrNull ?: return@filter false
                    HostGuard.isAllowed(u) && HostGuard.sameOrigin(u, conn.url)
                },
            )
        }
        return JsonObject(out)
    }

    private fun text(v: JsonElement?): String? =
        (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }
}
