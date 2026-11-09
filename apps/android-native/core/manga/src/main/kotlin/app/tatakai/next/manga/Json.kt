// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.longOrNull
import java.io.File

// Tolerant JSON readers shared by the private API clients of :core:manga and :core:novel (AniList, MangaUpdates, MangaDex). Every reader
// returns null instead of throwing on a missing or mistyped field: the remote APIs add and drop fields without notice.

/** Lenient JSON of the module (stores and API parsing). */
val ReaderJson: Json = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = true
    isLenient = true
    coerceInputValues = true
}

fun JsonElement?.obj(): JsonObject? = this as? JsonObject

fun JsonElement?.arr(): JsonArray? = this as? JsonArray

fun JsonObject?.sub(key: String): JsonObject? = this?.get(key) as? JsonObject

fun JsonObject?.list(key: String): List<JsonObject> = (this?.get(key) as? JsonArray)?.mapNotNull { it as? JsonObject }.orEmpty()

fun JsonObject?.strings(key: String): List<String> =
    (this?.get(key) as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }.orEmpty()

fun JsonObject?.str(key: String): String? {
    val p = this?.get(key) as? JsonPrimitive ?: return null
    if (p is JsonNull) return null
    return p.content.takeIf { it.isNotBlank() }
}

fun JsonObject?.int(key: String): Int? {
    val p = this?.get(key) as? JsonPrimitive ?: return null
    if (p is JsonNull) return null
    return p.intOrNull ?: p.doubleOrNull?.toInt() ?: p.content.trim().toIntOrNull()
}

fun JsonObject?.long(key: String): Long? {
    val p = this?.get(key) as? JsonPrimitive ?: return null
    if (p is JsonNull) return null
    return p.longOrNull ?: p.doubleOrNull?.toLong() ?: p.content.trim().toLongOrNull()
}

fun JsonObject?.double(key: String): Double? {
    val p = this?.get(key) as? JsonPrimitive ?: return null
    if (p is JsonNull) return null
    return p.doubleOrNull ?: p.content.trim().toDoubleOrNull()
}

fun JsonObject?.bool(key: String): Boolean {
    val p = this?.get(key) as? JsonPrimitive ?: return false
    if (p is JsonNull) return false
    return p.booleanOrNull ?: false
}

/**
 * One JSON document on disk (`storeDir/<name>.json`) with an in-memory copy. Writes go to a temp file and are renamed over the old one
 * so a crash never leaves half a file; a corrupt or missing file reads as [default]. Thread-safe through a [Mutex].
 */
class JsonFileStore<T>(
    private val file: File,
    private val serializer: KSerializer<T>,
    private val default: () -> T,
) {
    private val mutex = Mutex()
    private var cached: T? = null

    suspend fun read(): T = mutex.withLock { load() }

    suspend fun update(transform: (T) -> T): T = mutex.withLock {
        val next = transform(load())
        save(next)
        next
    }

    suspend fun write(value: T) = mutex.withLock { save(value) }

    suspend fun clear() = mutex.withLock {
        cached = null
        runCatching { file.delete() }
    }

    private fun load(): T {
        cached?.let { return it }
        val value = runCatching {
            if (file.isFile) ReaderJson.decodeFromString(serializer, file.readText()) else null
        }.getOrNull() ?: default()
        cached = value
        return value
    }

    private fun save(value: T) {
        cached = value
        runCatching {
            file.parentFile?.mkdirs()
            val tmp = File(file.parentFile, file.name + ".tmp")
            tmp.writeText(ReaderJson.encodeToString(serializer, value))
            if (!tmp.renameTo(file)) {
                file.delete()
                tmp.renameTo(file)
            }
        }
    }
}

/** Safe file / directory name for an id that may contain `/`, `:` or `?`. */
fun safeFileName(id: String): String {
    val cleaned = id.map { c -> if (c.isLetterOrDigit() || c == '-' || c == '_' || c == '.') c else '_' }.joinToString("")
    val trimmed = cleaned.take(80)
    return if (trimmed.length == cleaned.length && trimmed == id) trimmed else "${trimmed}_${Integer.toHexString(id.hashCode())}"
}
