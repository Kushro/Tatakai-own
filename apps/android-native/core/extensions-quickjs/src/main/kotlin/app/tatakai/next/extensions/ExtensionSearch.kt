// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtensionHomeItem
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.1, worker o): result parsing of the `custom/search` route
 * (contracts/extension-host/routes.v1.json: `{ namespace, results: CustomMediaCard[], hasNextPage? }`, `CustomMediaCard = { id, title,
 * image?, subtitle?, badge? }`). Bundles that return a bare array (wrapped by the host as `result`) or `items` are accepted too.
 */
internal object ExtensionSearch {
    /** Only `watch` custom sources take part in anime Search (`read` sources are manga, wave B). */
    const val WATCH_KIND = "watch"

    private fun JsonObject.text(key: String): String? =
        (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }

    private fun JsonObject.idText(key: String): String? =
        (this[key] as? JsonPrimitive)?.content?.trim()?.takeIf { it.isNotEmpty() && it != "null" }

    fun cards(body: JsonObject): List<ExtensionHomeItem> {
        val list: JsonElement? = body["results"] ?: body["items"] ?: body["result"]
        return (list as? JsonArray).orEmpty().mapNotNull { el ->
            val card = el as? JsonObject ?: return@mapNotNull null
            val title = card.text("title") ?: return@mapNotNull null
            ExtensionHomeItem(id = card.idText("id") ?: title, title = title, poster = card.text("image") ?: card.text("poster"))
        }
    }

    /** Results of several custom sources merged in source order, one card per title (case-insensitive). */
    fun merge(perSource: List<List<ExtensionHomeItem>>): List<ExtensionHomeItem> =
        perSource.flatten().distinctBy { it.title.lowercase() }
}
