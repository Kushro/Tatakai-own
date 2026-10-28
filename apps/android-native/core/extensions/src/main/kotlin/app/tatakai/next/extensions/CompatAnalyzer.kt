// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

/** `full | partial | unsupported`, computed at install time (globals.v1.md section 4). */
enum class CompatLevel { FULL, PARTIAL, UNSUPPORTED }

data class CompatReport(
    val level: CompatLevel,
    val specifiers: List<String>,
    val unsupported: List<String>,
    val stubs: List<String>,
    val dynamic: Boolean,
    val reason: String?,
)

/** Static analysis of `bundle.js`; same result as contracts/extension-host/compat.mjs for the same text. */
object CompatAnalyzer {
    val SUPPORTED_MODULES = listOf("crypto", "buffer", "events", "url", "path", "querystring", "util")
    val STUB_MODULES = listOf(
        "os", "http", "https", "stream", "zlib", "string_decoder", "assert", "timers", "readline", "perf_hooks",
        "async_hooks", "module", "tty",
    )
    val UNSUPPORTED_BUILTINS = listOf(
        "fs", "child_process", "net", "tls", "dgram", "cluster", "worker_threads", "vm", "dns", "http2", "inspector", "v8", "repl",
    )

    private val requireLiteral = Regex("""\brequire\s*\(\s*(['"`])([^'"`\r\n]+)\1\s*\)""")
    private val requireAny = Regex("""\brequire\s*\(""")
    private val requireResolve = Regex("""\brequire\s*\.\s*resolve\b""")
    private val dynamicImport = Regex("""(?<![\w.${'$'}])import\s*\(""")

    fun analyze(source: String): CompatReport {
        val specifiers = requireLiteral.findAll(source).map { it.groupValues[2].removePrefix("node:") }.toList()
        val anyCount = requireAny.findAll(source).count()
        val dynamic = anyCount > specifiers.size || requireResolve.containsMatchIn(source) || dynamicImport.containsMatchIn(source)

        val unique = specifiers.distinct()
        val unsupported = unique.filter { it !in SUPPORTED_MODULES && it !in STUB_MODULES }
        val stubs = unique.filter { it in STUB_MODULES }

        if (unsupported.isNotEmpty()) {
            val first = unsupported.first()
            val kind = if (first in UNSUPPORTED_BUILTINS) "Node module" else "module"
            return CompatReport(
                CompatLevel.UNSUPPORTED, unique, unsupported, stubs, dynamic,
                "Uses $kind '$first' which the native runtime does not provide",
            )
        }
        if (stubs.isNotEmpty() || dynamic) {
            return CompatReport(
                CompatLevel.PARTIAL, unique, unsupported, stubs, dynamic,
                if (stubs.isNotEmpty()) "Uses stubbed Node module '${stubs.first()}'" else "Uses dynamic require/import",
            )
        }
        return CompatReport(CompatLevel.FULL, unique, unsupported, stubs, dynamic, null)
    }
}
