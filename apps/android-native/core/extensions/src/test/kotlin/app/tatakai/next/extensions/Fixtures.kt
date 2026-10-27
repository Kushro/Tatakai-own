// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import java.io.File

/** Locates `contracts/fixtures/extensions` by walking up from the working directory (or `TATAKAI_CONTRACTS_DIR`). */
object Fixtures {
    val root: File by lazy {
        System.getenv("TATAKAI_CONTRACTS_DIR")?.let { File(it, "fixtures/extensions") }?.takeIf { it.isDirectory }
            ?: generateSequence(File(System.getProperty("user.dir")).absoluteFile) { it.parentFile }
                .map { File(it, "contracts/fixtures/extensions") }
                .firstOrNull { it.isDirectory }
            ?: error("contracts/fixtures/extensions not found from ${System.getProperty("user.dir")}")
    }

    fun text(path: String): String = File(root, path).readText(Charsets.UTF_8)

    fun kai(name: String): File = File(root, "packed/$name.kai")
}
