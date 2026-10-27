// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.InputStream
import java.util.zip.ZipInputStream

/** A validated `.kai` package: manifest, bundle text and compat report. Renderer entries are never extracted. */
class KaiPackage(
    val manifest: ExtensionManifest,
    val manifestText: String,
    val bundle: String,
    val compat: CompatReport,
) {
    /** Unsupported extensions install but are forced disabled. */
    val enabledByDefault: Boolean get() = compat.level != CompatLevel.UNSUPPORTED
}

class KaiException(message: String) : Exception(message)

object KaiReader {
    const val MAX_BUNDLE_BYTES = 10 * 1024 * 1024
    const val MAX_ENTRIES = 64
    const val MAX_TOTAL_BYTES = 32L * 1024 * 1024
    const val MAX_MANIFEST_BYTES = 256 * 1024
    const val MANIFEST_FILE = "manifest.json"
    const val BUNDLE_FILE = "bundle.js"

    /**
     * Reads and validates a `.kai` ZIP. Nothing is written to disk here.
     * @throws KaiException / [ManifestException] with messages naming the failing field or entry.
     */
    fun read(input: InputStream): KaiPackage {
        var manifestBytes: ByteArray? = null
        var bundleBytes: ByteArray? = null
        var entries = 0
        var total = 0L
        try {
            ZipInputStream(input).use { zip ->
                while (true) {
                    val entry = zip.nextEntry ?: break
                    entries++
                    if (entries > MAX_ENTRIES) throw KaiException("Package has more than $MAX_ENTRIES entries")
                    val name = entry.name
                    if (isUnsafePath(name)) throw KaiException("Package entry escapes the archive: rejected")
                    val keep = name == MANIFEST_FILE || name == BUNDLE_FILE
                    val limit = when (name) {
                        MANIFEST_FILE -> MAX_MANIFEST_BYTES.toLong()
                        BUNDLE_FILE -> MAX_BUNDLE_BYTES.toLong()
                        else -> MAX_TOTAL_BYTES
                    }
                    val out = ByteArrayOutputStream()
                    val buf = ByteArray(16 * 1024)
                    var read = 0L
                    while (true) {
                        val n = zip.read(buf)
                        if (n < 0) break
                        read += n
                        if (read > limit) {
                            throw KaiException(
                                if (name == BUNDLE_FILE) "bundle.js is larger than $MAX_BUNDLE_BYTES bytes" else "Package entry is too large",
                            )
                        }
                        if (total + read > MAX_TOTAL_BYTES) throw KaiException("Package uncompressed size exceeds $MAX_TOTAL_BYTES bytes")
                        // Only the two known files are kept in memory; everything else is counted and dropped.
                        if (keep) out.write(buf, 0, n)
                    }
                    total += read
                    if (name == MANIFEST_FILE) manifestBytes = out.toByteArray()
                    if (name == BUNDLE_FILE) bundleBytes = out.toByteArray()
                }
            }
        } catch (e: IOException) {
            throw KaiException("Package is not a valid ZIP file")
        }
        val manifestRaw = manifestBytes ?: throw KaiException("manifest.json is missing")
        val bundleRaw = bundleBytes ?: throw KaiException("bundle.js is missing")
        val manifestText = manifestRaw.toString(Charsets.UTF_8)
        val manifest = ManifestParser.parse(manifestText)
        val bundle = bundleRaw.toString(Charsets.UTF_8)
        return KaiPackage(manifest, manifestText, bundle, CompatAnalyzer.analyze(bundle))
    }

    fun isUnsafePath(name: String): Boolean {
        if (name.startsWith("/") || name.startsWith("\\")) return true
        if (Regex("^[A-Za-z]:").containsMatchIn(name)) return true
        return name.split('/', '\\').any { it == ".." }
    }
}
