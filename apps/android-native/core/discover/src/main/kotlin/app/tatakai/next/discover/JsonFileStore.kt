// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.json.Json
import java.io.File

internal val DiscoverJson = Json { ignoreUnknownKeys = true; encodeDefaults = true }

/** Tiny atomic JSON file (write to `.tmp`, rename) for the private stores of :core:discover. Corrupt / missing = [default]. */
internal class JsonFileStore<T>(private val file: File, private val serializer: KSerializer<T>, private val default: () -> T) {
    fun readBlocking(): T = try {
        if (file.isFile) DiscoverJson.decodeFromString(serializer, file.readText()) else default()
    } catch (_: Exception) {
        default()
    }

    suspend fun read(): T = withContext(Dispatchers.IO) { readBlocking() }

    suspend fun write(value: T): Unit = withContext(Dispatchers.IO) {
        file.parentFile?.mkdirs()
        val tmp = File(file.path + ".tmp")
        tmp.writeText(DiscoverJson.encodeToString(serializer, value))
        if (!tmp.renameTo(file)) {
            file.delete()
            tmp.renameTo(file)
        }
        Unit
    }
}
