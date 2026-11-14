// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.MangaRouteChapter
import app.tatakai.next.model.MangaRouteQuery
import app.tatakai.next.model.ProviderEntry
import app.tatakai.next.model.ProviderItem
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t; checklist ext.custom-providers.*): tolerant parsers
 * of the custom-provider routes (contracts/extension-host/routes.v1.json `custom/home` `CustomHomeSection[]`, `custom/search`
 * `CustomMediaCard[]`, `custom/info` `CustomInfoResult { id, title, image?, description?, meta?, entries: { id, label, number? }[] }`,
 * `custom/read` `{ pages: { pageNumber, imageUrl, headers? }[] }`) and of the same data answered by a PC extension server (`/invoke` ops
 * `browse` / `search` / `info` / `episodes` / `chapters` / `pages`, whose rows use `name` / `poster` / `videos` / `chapters`). Pure JVM,
 * nothing throws: a field that does not fit is skipped.
 */
object ProviderParsers {
    /** `sections[]` (or a bare `items[]` / array) -> rows titled by the section, or [fallbackTitle] for an untitled list. */
    fun homeRows(extensionId: String, sourceId: String, fallbackTitle: String, body: JsonElement?): List<ExtensionHomeRow> {
        val sections: List<JsonElement> = when (body) {
            is JsonArray -> if (body.firstOrNull()?.let { it is JsonObject && it["items"] is JsonArray } == true) body else listOf(wrapItems(body))
            is JsonObject -> (body["sections"] as? JsonArray)
                ?: (body["result"] as? JsonArray)?.let { arr ->
                    if (arr.firstOrNull()?.let { it is JsonObject && it["items"] is JsonArray } == true) arr else JsonArray(listOf(wrapItems(arr)))
                }
                ?: listOfNotNull(listKey(body)?.let { wrapItems(it) })
            else -> emptyList()
        }
        return sections.mapNotNull { section ->
            val o = section as? JsonObject ?: return@mapNotNull null
            val items = cards(o["items"])
            if (items.isEmpty()) null else ExtensionHomeRow(extensionId, sourceId, o.text("title") ?: o.text("name") ?: fallbackTitle, items)
        }
    }

    /** `results[]` / `items[]` / `metas[]` / `result[]` / a bare array of cards -> items (one per id). */
    fun cards(body: JsonElement?): List<ExtensionHomeItem> {
        val list: JsonArray = when (body) {
            is JsonArray -> body
            is JsonObject -> listKey(body) ?: return emptyList()
            else -> return emptyList()
        }
        return list.mapNotNull { el ->
            val card = el as? JsonObject ?: return@mapNotNull null
            val title = card.text("title") ?: card.text("name") ?: return@mapNotNull null
            ExtensionHomeItem(
                id = card.idText("id") ?: card.idText("url") ?: title,
                title = title,
                poster = card.text("image") ?: card.text("poster") ?: card.text("cover") ?: card.text("thumbnail"),
            )
        }.distinctBy { it.id }
    }

    /** Whether a search answer says there is another page (`hasNextPage` / `hasNext`). */
    fun hasNextPage(body: JsonElement?): Boolean =
        (body as? JsonObject)?.let { o -> o.flag("hasNextPage") ?: o.flag("hasNext") } ?: false

    /**
     * `CustomInfoResult` -> [ProviderItem]. [kind] is the source kind (`watch` | `read` | `novel`), given to every entry. Entries come
     * from `entries` / `episodes` / `chapters` / `videos`; [extraEntries] (a separate `episodes` / `chapters` answer of a server) is used
     * when the info has none.
     */
    fun info(itemId: String, kind: String, body: JsonElement?, extraEntries: JsonElement? = null): ProviderItem? {
        val o = unwrap(body) ?: return null
        val title = o.text("title") ?: o.text("name") ?: return null
        val meta = o["meta"] as? JsonObject
        val genres = stringList(o["genres"]).ifEmpty { stringList(meta?.get("genres")) }.ifEmpty {
            meta?.text("genres")?.split(',')?.map { it.trim() }?.filter { it.isNotEmpty() }.orEmpty()
        }
        var entries = entries(kind, o["entries"] ?: o["episodes"] ?: o["chapters"] ?: o["videos"])
        if (entries.isEmpty() && extraEntries != null) {
            entries = entries(kind, (extraEntries as? JsonObject)?.let { it["chapters"] ?: it["episodes"] ?: it["entries"] ?: it["result"] } ?: extraEntries)
        }
        return ProviderItem(
            id = o.idText("id") ?: itemId,
            title = title,
            poster = o.text("image") ?: o.text("poster") ?: o.text("cover"),
            banner = o.text("banner") ?: o.text("background"),
            description = o.text("description") ?: o.text("synopsis") ?: meta?.text("description"),
            status = o.text("status") ?: meta?.text("status") ?: meta?.text("Status"),
            genres = genres,
            entries = entries,
        )
    }

    /** `{ id, label | title | name, number | episode | chapter?, releasedAt?, lang? }[]` -> entries in the order given. */
    fun entries(kind: String, el: JsonElement?): List<ProviderEntry> {
        val arr = el as? JsonArray ?: return emptyList()
        return arr.mapNotNull { item ->
            val o = item as? JsonObject ?: return@mapNotNull null
            val id = o.idText("id") ?: o.idText("url") ?: o.idText("key") ?: return@mapNotNull null
            ProviderEntry(
                id = id,
                kind = kind,
                number = o.number("number") ?: o.number("episode") ?: o.number("chapter") ?: o.number("chapterNumber"),
                title = o.text("label") ?: o.text("title") ?: o.text("name"),
                releasedAtMs = o.epochMs("releasedAt") ?: o.epochMs("released") ?: o.epochMs("date") ?: o.epochMs("uploadDate"),
                lang = o.text("lang") ?: o.text("language"),
            )
        }.distinctBy { it.id }
    }

    /**
     * `custom/read` / server `pages` -> pages (image URL + headers), in `pageNumber` order. For a `novel` entry the answer carries text
     * (`content` / `text` / `paragraphs[]`): each paragraph becomes one [ChapterPage] whose `url` is the paragraph text (the caller joins).
     */
    fun pages(kind: String, body: JsonElement?): List<ChapterPage> {
        if (kind == "novel") {
            val o = unwrap(body)
            val paragraphs = stringList(o?.get("paragraphs")).ifEmpty {
                (o?.text("content") ?: o?.text("text") ?: (body as? JsonPrimitive)?.contentOrNull)
                    ?.let(::paragraphsOf).orEmpty()
            }
            if (paragraphs.isNotEmpty()) return paragraphs.mapIndexed { i, p -> ChapterPage(i, p) }
        }
        val list: JsonArray = when (body) {
            is JsonArray -> body
            is JsonObject -> (body["pages"] as? JsonArray) ?: (body["result"] as? JsonArray) ?: return emptyList()
            else -> return emptyList()
        }
        val raw = list.mapIndexedNotNull { i, el ->
            when (el) {
                is JsonPrimitive -> el.contentOrNull?.takeIf { isHttp(it) }?.let { Triple(i.toDouble(), it, emptyMap<String, String>()) }
                is JsonObject -> {
                    val url = el.text("imageUrl") ?: el.text("url") ?: el.text("image") ?: return@mapIndexedNotNull null
                    if (!isHttp(url)) return@mapIndexedNotNull null
                    Triple(el.number("pageNumber") ?: el.number("index") ?: i.toDouble(), url, headers(el["headers"]))
                }
                else -> null
            }
        }
        return raw.sortedBy { it.first }.mapIndexed { i, (_, url, headers) -> ChapterPage(i, url, headers) }
    }

    /**
     * BUG-099: a `manga/chapters` answer (same shapes as desktop `tk_core::manga::parse_ext_chapters`): a bare array, `results`
     * (debug variant), `chapters` (a PC server's progressive answer) or `data`, of either merged entries
     * `{ number, title, volume, sources: [{ provider, chapterKey, language, scanlator, releaseDate }] }` or flat entries
     * `{ chapterKey | id, number, title, volume, language | lang, scanlator | group, releaseDate | date }`. The provider is `provider`,
     * else the `chapterKey` prefix (`mangadex:abc`), else [extensionId]. Duplicate keys of one provider are dropped.
     */
    fun mangaRouteChapters(extensionId: String, body: JsonElement?): List<MangaRouteChapter> {
        val list: JsonArray = when (body) {
            is JsonArray -> body
            is JsonObject -> (body["results"] ?: body["chapters"] ?: body["result"] ?: body["data"]) as? JsonArray ?: return emptyList()
            else -> return emptyList()
        }
        val out = ArrayList<MangaRouteChapter>()
        val seen = HashSet<String>()
        fun add(o: JsonObject, parent: JsonObject?) {
            val key = o.idText("chapterKey") ?: o.idText("id") ?: return
            val provider = o.text("provider") ?: key.substringBefore(':', "").takeIf { it.isNotEmpty() } ?: extensionId
            if (!seen.add("$provider|$key")) return
            val number = listOfNotNull(parent?.number("number"), parent?.number("chapter"), o.number("number"), o.number("chapter"))
                .firstOrNull { it > 0 }
            out += MangaRouteChapter(
                provider = provider,
                key = key,
                number = number,
                volume = (o.volume("volume") ?: parent?.volume("volume")),
                title = o.text("title") ?: parent?.text("title"),
                lang = (o.text("language") ?: o.text("lang"))?.lowercase(java.util.Locale.ROOT)?.substringBefore('-')?.takeIf { it.length in 2..3 },
                group = o.text("scanlator") ?: o.text("group"),
                releasedAtMs = o.epochMs("releaseDate") ?: o.epochMs("date"),
            )
        }
        for (el in list) {
            val e = el as? JsonObject ?: continue
            val sources = e["sources"] as? JsonArray
            if (sources != null) sources.forEach { s -> (s as? JsonObject)?.let { add(it, e) } } else add(e, null)
        }
        return out
    }

    /** `manga/chapters` params for [query] (as desktop `fetch_ext`): `anilistId`, `malId`, `title` (the first one) and `titles`. */
    fun mangaRouteParams(query: MangaRouteQuery): JsonObject = JsonObject(
        buildMap {
            query.anilistId?.let { put("anilistId", JsonPrimitive(it)) }
            query.malId?.let { put("malId", JsonPrimitive(it)) }
            query.titles.firstOrNull()?.let { put("title", JsonPrimitive(it)) }
            put("titles", JsonArray(query.titles.map { JsonPrimitive(it) }))
        },
    )

    /** `manga/pages` params for a `chapterKey` (`<provider>:<providerChapterId>`), as desktop `fetch_pages`. */
    fun mangaPagesParams(chapterKey: String): JsonObject {
        val provider = chapterKey.substringBefore(':', "")
        return JsonObject(
            buildMap {
                put("chapterKey", JsonPrimitive(chapterKey))
                put("provider", if (provider.isEmpty()) JsonNull else JsonPrimitive(provider))
                put("providerChapterId", JsonPrimitive(chapterKey.substringAfter(':')))
            },
        )
    }

    /** Blank-line separated paragraphs (single newlines kept inside a paragraph); HTML `<p>` tags are split too. */
    fun paragraphsOf(text: String): List<String> =
        text.replace(Regex("(?i)</p>\\s*<p[^>]*>"), "\n\n").replace(Regex("(?i)</?p[^>]*>"), "")
            .replace(Regex("(?i)<br\\s*/?>"), "\n")
            .split(Regex("\\n\\s*\\n")).map { it.trim() }.filter { it.isNotEmpty() }

    // ---- helpers ----------------------------------------------------------------------------------------------------------------

    private fun wrapItems(items: JsonArray): JsonObject = JsonObject(mapOf("items" to items))

    private fun listKey(o: JsonObject): JsonArray? =
        (o["results"] ?: o["items"] ?: o["metas"] ?: o["result"] ?: o["list"]) as? JsonArray

    /** `{ result: {...} }` / `{ data: {...} }` envelopes of some bundles -> the inner object. */
    private fun unwrap(body: JsonElement?): JsonObject? {
        val o = body as? JsonObject ?: return null
        if (o.text("title") != null || o.text("name") != null) return o
        return (o["result"] as? JsonObject) ?: (o["data"] as? JsonObject) ?: (o["meta"] as? JsonObject)?.takeIf { it.text("name") != null } ?: o
    }

    private fun headers(el: JsonElement?): Map<String, String> =
        (el as? JsonObject)?.mapNotNull { (k, v) -> (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.let { k to it } }?.toMap().orEmpty()

    private fun stringList(el: JsonElement?): List<String> =
        (el as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content?.trim()?.takeIf { s -> s.isNotEmpty() } }

    private fun isHttp(url: String): Boolean = url.startsWith("https://", ignoreCase = true) || url.startsWith("http://", ignoreCase = true)

    private fun JsonObject.text(key: String): String? =
        (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }

    private fun JsonObject.idText(key: String): String? =
        (this[key] as? JsonPrimitive)?.takeUnless { it is JsonNull }?.content?.trim()?.takeIf { it.isNotEmpty() && it != "null" }

    private fun JsonObject.number(key: String): Double? {
        val p = this[key] as? JsonPrimitive ?: return null
        if (p is JsonNull) return null
        return p.doubleOrNull ?: p.content.trim().toDoubleOrNull()
    }

    /** A volume number or text (`3`, `"Vol. 3"`); null when missing or 0. */
    private fun JsonObject.volume(key: String): Int? {
        val p = this[key] as? JsonPrimitive ?: return null
        if (p is JsonNull) return null
        val n = p.doubleOrNull ?: Regex("(\\d+)").find(p.content)?.groupValues?.get(1)?.toDoubleOrNull() ?: return null
        return n.toInt().takeIf { it in 1 until 100_000 }
    }

    private fun JsonObject.flag(key: String): Boolean? = (this[key] as? JsonPrimitive)?.takeUnless { it.isString }?.contentOrNull?.toBooleanStrictOrNull()

    /** Epoch seconds / ms or ISO-8601 -> epoch ms. */
    private fun JsonObject.epochMs(key: String): Long? {
        val p = this[key] as? JsonPrimitive ?: return null
        if (p is JsonNull) return null
        p.longOrNull?.let { return if (it < 100_000_000_000L) it * 1000 else it }
        return runCatching { java.time.Instant.parse(p.content.trim()).toEpochMilli() }.getOrNull()
            ?: runCatching { java.time.LocalDate.parse(p.content.trim().take(10)).atStartOfDay(java.time.ZoneOffset.UTC).toInstant().toEpochMilli() }.getOrNull()
    }
}
