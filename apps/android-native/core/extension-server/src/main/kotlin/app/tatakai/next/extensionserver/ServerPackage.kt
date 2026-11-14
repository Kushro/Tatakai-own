// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensions.KaiReader
import java.io.ByteArrayInputStream
import java.security.MessageDigest

// BUG-055: "Actualizar desde la PC" downloads the `.kai` the PC's extension server has installed and installs it on the phone. The package
// is checked here before the local installer (which validates it again and records its SHA-256) sees it.

/** The bytes of `GET /extensions/{id}/package` and the server's `X-Tatakai-Bundle-Sha256`. */
class ServerPackage(val bytes: ByteArray, val bundleSha256: String?)

/** Why a downloaded package is refused. */
enum class PackageProblem {
    /** Not a readable `.kai` (bad ZIP, missing manifest or bundle, invalid manifest). */
    UNREADABLE,

    /** The manifest is another extension than the one asked for. */
    WRONG_EXTENSION,

    /** The bundle's SHA-256 does not match what the server announced (header or extension list), or nothing was announced. */
    HASH_MISMATCH,
}

/**
 * Checks [pkg] against [expectedId] and the SHA-256 the server announced: the `bundle.js` inside must hash to the response header and,
 * when the extension list carried one ([listedSha256]), to that too. Returns null when the package can be installed.
 */
fun verifyServerPackage(pkg: ServerPackage, expectedId: String, listedSha256: String?): PackageProblem? {
    val kai = try {
        KaiReader.read(ByteArrayInputStream(pkg.bytes))
    } catch (_: Exception) {
        return PackageProblem.UNREADABLE
    }
    if (kai.manifest.id != expectedId) return PackageProblem.WRONG_EXTENSION
    val actual = sha256Hex(kai.bundle.toByteArray(Charsets.UTF_8))
    val header = pkg.bundleSha256?.lowercase() ?: return PackageProblem.HASH_MISMATCH
    if (header != actual) return PackageProblem.HASH_MISMATCH
    if (listedSha256 != null && listedSha256.lowercase() != actual) return PackageProblem.HASH_MISMATCH
    return null
}

internal fun sha256Hex(bytes: ByteArray): String =
    MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(it) }
