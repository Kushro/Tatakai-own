// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
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
