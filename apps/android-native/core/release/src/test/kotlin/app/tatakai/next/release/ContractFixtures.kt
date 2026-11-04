// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.io.File

/** Locates `contracts/fixtures/<sub>`: `TATAKAI_CONTRACTS_DIR` (the `contracts` dir) or walking up from `user.dir`. */
object ContractFixtures {
    private val contracts: File by lazy {
        System.getenv("TATAKAI_CONTRACTS_DIR")?.let { File(it) }?.takeIf { File(it, "fixtures").isDirectory }?.let { return@lazy it }
        var d: File? = File(System.getProperty("user.dir")).absoluteFile
        while (d != null) {
            val c = File(d, "contracts")
            if (File(c, "fixtures/release").isDirectory) return@lazy c
            d = d.parentFile
        }
        error("contracts/fixtures/release not found above ${System.getProperty("user.dir")} (set TATAKAI_CONTRACTS_DIR)")
    }

    fun text(vararg path: String): String = File(contracts, "fixtures/" + path.joinToString("/")).readText(Charsets.UTF_8)

    fun element(vararg path: String): JsonElement = Json.parseToJsonElement(text(*path))
}
