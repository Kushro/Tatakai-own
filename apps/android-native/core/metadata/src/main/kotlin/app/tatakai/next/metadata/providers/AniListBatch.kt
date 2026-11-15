// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.model.RequestPriority
import app.tatakai.next.model.RequestPriorityElement
import app.tatakai.next.model.currentRequestPriority
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * 0.3.1: several AniList queries in ONE request with GraphQL aliases (pure build / split).
 *
 * Each part is a single-field operation (`query ($a: Int) { Page(…) { … } } fragment …`). [build] renames its variables
 * (`$page` → `$q0_page`), aliases its field (`q0: Page(…)`), merges the fragments (same name = same text, else not batchable) and
 * the variables. [split] turns the answer back into one document per part shaped exactly like the single request's answer
 * (`{"data": {"Page": …}, "errors": […]}`), so every mapper keeps working unchanged.
 */
internal object AliasBatch {
    data class Part(val query: String, val variables: JsonObject)

    data class Built(val query: String, val variables: JsonObject, val aliases: List<String>, val fields: List<String>)

    /** A parsed single-field operation. */
    data class Op(val declarations: String, val field: String, val selection: String, val fragments: Map<String, String>)

    private val VARIABLE = Regex("""\$(\w+)""")
    private val FRAGMENT = Regex("""fragment\s+(\w+)\s+on\s+\w+\s*\{""")
    private val NAME = Regex("""[_A-Za-z]\w*""")

    fun alias(index: Int): String = "q$index"

    /** Parses `query [Name] [(decls)] { Field[(args)] [{…}] } fragment…`; null when it is not a single-field query. */
    fun parse(query: String): Op? {
        val s = query.trim()
        if (!s.startsWith("query")) return null
        var i = "query".length
        i = skipWs(s, i)
        NAME.matchAt(s, i)?.let { i = skipWs(s, it.range.last + 1) }
        var declarations = ""
        if (i < s.length && s[i] == '(') {
            val end = matching(s, i, '(', ')') ?: return null
            declarations = s.substring(i + 1, end).trim()
            i = skipWs(s, end + 1)
        }
        if (i >= s.length || s[i] != '{') return null
        val bodyEnd = matching(s, i, '{', '}') ?: return null
        val body = s.substring(i + 1, bodyEnd).trim()
        val fragments = fragments(s.substring(bodyEnd + 1)) ?: return null
        // exactly one top-level field, without an alias of its own
        val name = NAME.matchAt(body, 0) ?: return null
        var j = skipWs(body, name.range.last + 1)
        if (j < body.length && body[j] == ':') return null
        if (j < body.length && body[j] == '(') j = skipWs(body, (matching(body, j, '(', ')') ?: return null) + 1)
        if (j < body.length && body[j] == '{') j = skipWs(body, (matching(body, j, '{', '}') ?: return null) + 1)
        if (j != body.length) return null
        return Op(declarations, name.value, body, fragments)
    }

    private fun fragments(rest: String): Map<String, String>? {
        val out = LinkedHashMap<String, String>()
        var i = skipWs(rest, 0)
        while (i < rest.length) {
            val m = FRAGMENT.matchAt(rest, i) ?: return null
            val open = m.range.last
            val end = matching(rest, open, '{', '}') ?: return null
            out[m.groupValues[1]] = rest.substring(i, end + 1).trim()
            i = skipWs(rest, end + 1)
        }
        return out
    }

    private fun skipWs(s: String, from: Int): Int {
        var i = from
        while (i < s.length && (s[i].isWhitespace() || s[i] == ',')) i++
        return i
    }

    private fun matching(s: String, open: Int, o: Char, c: Char): Int? {
        var depth = 0
        for (k in open until s.length) {
            when (s[k]) {
                o -> depth++
                c -> if (--depth == 0) return k
            }
        }
        return null
    }

    /** One aliased query for [parts]; null when a part is not a single-field query or two parts define a fragment differently. */
    fun build(parts: List<Part>): Built? {
        if (parts.isEmpty()) return null
        val ops = parts.map { parse(it.query) ?: return null }
        val fragments = LinkedHashMap<String, String>()
        for (op in ops) for ((name, text) in op.fragments) {
            val known = fragments.putIfAbsent(name, text)
            if (known != null && known != text) return null
        }
        val declarations = mutableListOf<String>()
        val selections = mutableListOf<String>()
        val variables = LinkedHashMap<String, JsonElement>()
        val aliases = mutableListOf<String>()
        ops.forEachIndexed { index, op ->
            val a = alias(index)
            aliases += a
            fun rename(text: String) = VARIABLE.replace(text) { "\$${a}_${it.groupValues[1]}" }
            if (op.declarations.isNotEmpty()) declarations += rename(op.declarations)
            selections += "$a: ${rename(op.selection)}"
            for ((k, v) in parts[index].variables) variables["${a}_$k"] = v
        }
        val header = if (declarations.isEmpty()) "query" else "query (${declarations.joinToString(", ")})"
        val query = buildString {
            append(header).append(" {\n")
            selections.forEach { append("  ").append(it).append('\n') }
            append("}\n")
            fragments.values.forEach { append(it).append('\n') }
        }
        return Built(query, JsonObject(variables), aliases, ops.map { it.field })
    }

    /**
     * One document per part, shaped like the part's own answer: its aliased field under its real name, and the errors whose `path`
     * starts with its alias (errors without a path go to every part). A null `data` stays null for every part.
     */
    fun split(doc: JsonElement, built: Built): List<JsonElement> {
        val root = doc as? JsonObject
        val data = root?.get("data") as? JsonObject
        val errors = (root?.get("errors") as? JsonArray).orEmpty()
        fun pathHead(e: JsonElement): String? =
            ((e as? JsonObject)?.get("path") as? JsonArray)?.firstOrNull()?.let { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }
        return built.aliases.mapIndexed { index, alias ->
            val mine = errors.filter { pathHead(it).let { h -> h == null || h == alias } }
            val fields = mutableMapOf<String, JsonElement>()
            fields["data"] = if (data == null) JsonNull else JsonObject(mapOf(built.fields[index] to (data[alias] ?: JsonNull)))
            if (mine.isNotEmpty()) fields["errors"] = JsonArray(mine)
            JsonObject(fields)
        }
    }

    /** True when the batched answer failed as a whole (no `data` at all): its parts are worth retrying one by one. */
    fun failedAsWhole(doc: JsonElement): Boolean {
        val root = doc as? JsonObject ?: return true
        val data = root["data"]
        return data == null || data is JsonNull
    }
}

/**
 * 0.3.1: coalesces the AniList queries issued within [windowMs] of each other (Home opening its rows, the "Because you watched"
 * seeds, the watch order hops of both directions, the calendar pages) into aliased requests of at most [maxCost] (see [AliasBatch]).
 * Identical parts share one alias. A request that fails as a whole (GraphQL error without data, e.g. a complexity limit) is retried
 * part by part; an HTTP / transport failure fails every part (the gateway already retried what was worth it).
 *
 * [send] performs one HTTP request and returns the raw document (HTTP / transport failures throw; GraphQL errors stay in the
 * document so each part sees its own). A batch goes out with the most urgent priority of its parts.
 */
internal class AniListBatcher(
    private val send: suspend (query: String, variables: JsonObject) -> JsonElement,
    private val windowMs: Long = WINDOW_MS,
    private val maxCost: Int = MAX_COST,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    private class Pending(val part: AliasBatch.Part, val cost: Int, val priority: RequestPriority, val result: CompletableDeferred<JsonElement>) {
        val key: String = part.query + "\u0000" + part.variables.toString()
    }

    private val lock = Any()
    private val queue = mutableListOf<Pending>()
    private var scheduled = false

    /** Requests sent by this batcher (for tests / diagnostics). */
    @Volatile var requestsSent: Int = 0
        private set

    suspend fun run(query: String, variables: JsonObject, cost: Int = 1): JsonElement {
        val p = Pending(AliasBatch.Part(query, variables), cost.coerceAtLeast(1), currentRequestPriority(), CompletableDeferred())
        synchronized(lock) {
            queue += p
            if (!scheduled) {
                scheduled = true
                scope.launch {
                    delay(windowMs)
                    flush()
                }
            }
        }
        return p.result.await()
    }

    private fun flush() {
        val batch = synchronized(lock) {
            scheduled = false
            queue.toList().also { queue.clear() }
        }
        // identical parts share one alias
        val groups = batch.groupBy { it.key }.values.toList()
        for (chunk in chunks(groups)) scope.launch { sendChunk(chunk) }
    }

    /** Greedy chunks of at most [maxCost] (a part costlier than that goes alone). */
    private fun chunks(groups: List<List<Pending>>): List<List<List<Pending>>> {
        val out = mutableListOf<MutableList<List<Pending>>>()
        var current = mutableListOf<List<Pending>>()
        var cost = 0
        for (g in groups) {
            val c = g.first().cost
            if (current.isNotEmpty() && cost + c > maxCost) {
                out += current
                current = mutableListOf()
                cost = 0
            }
            current += g
            cost += c
        }
        if (current.isNotEmpty()) out += current
        return out
    }

    private fun priorityOf(groups: List<List<Pending>>): RequestPriority =
        if (groups.any { g -> g.any { it.priority == RequestPriority.INTERACTIVE } }) RequestPriority.INTERACTIVE else RequestPriority.BACKGROUND

    private suspend fun sendOne(group: List<Pending>) {
        val outcome = runCatching {
            withContext(RequestPriorityElement(priorityOf(listOf(group)))) {
                requestsSent++
                send(group.first().part.query, group.first().part.variables)
            }
        }
        group.forEach { p -> outcome.fold({ p.result.complete(it) }, { p.result.completeExceptionally(it) }) }
    }

    private suspend fun sendChunk(chunk: List<List<Pending>>) {
        if (chunk.size == 1) return sendOne(chunk.first())
        val built = AliasBatch.build(chunk.map { it.first().part })
        if (built == null) {
            chunk.forEach { sendOne(it) }
            return
        }
        val doc = try {
            withContext(RequestPriorityElement(priorityOf(chunk))) {
                requestsSent++
                send(built.query, built.variables)
            }
        } catch (e: CancellationException) {
            chunk.forEach { g -> g.forEach { it.result.cancel(e) } }
            throw e
        } catch (e: Throwable) {
            if (e.isWholeBatchGraphQlError()) {
                chunk.forEach { sendOne(it) }
            } else {
                chunk.forEach { g -> g.forEach { it.result.completeExceptionally(e) } }
            }
            return
        }
        if (AliasBatch.failedAsWhole(doc)) {
            chunk.forEach { sendOne(it) }
            return
        }
        AliasBatch.split(doc, built).forEachIndexed { i, part -> chunk[i].forEach { it.result.complete(part) } }
    }

    /** A batched request AniList refused as a whole (HTTP 400 / 413: validation, complexity, size): worth retrying part by part. */
    private fun Throwable.isWholeBatchGraphQlError(): Boolean {
        val e = this as? app.tatakai.next.metadata.ProviderException ?: return false
        return e.status == 400 || e.status == 413
    }

    companion object {
        const val WINDOW_MS = 30L
        const val MAX_COST = 16
    }
}
