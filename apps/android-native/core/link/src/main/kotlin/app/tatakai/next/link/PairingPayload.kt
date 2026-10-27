// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from Enigmata (app.enigmata.temple.pairing.TemplePairingPayload), same author, AGPL-3.0-or-later. See apps/android-native/THIRD_PARTY_NOTICES.md.
package app.tatakai.next.link

import java.net.URLDecoder
import java.util.Locale

/**
 * The pairing payload `tatakai-link:v1?h=<ipv4>&p=<port>&ca=<sha256 hex>&c=<10 Crockford>&x=<unix exp>&n=<PC name url-encoded>` shown by
 * the PC as text or QR (contracts/link/openapi.v1.json `x-pairing-payload`). `v0`, other schemes (e.g. `enigmata-temple:`), expired
 * and malformed payloads are rejected. [toString] never includes the code: it is a one-time secret.
 */
class PairingPayload private constructor(
    val host: String,
    val port: Int,
    val caSha256: String,
    val code: String,
    val expiresAtEpochSeconds: Long,
    val pcName: String,
) {
    /** `https://host:port`, the base of every route. */
    val baseUrl: String get() = "https://$host:$port"

    override fun toString(): String =
        "PairingPayload(host=$host, port=$port, ca=${caSha256.take(8)}..., expiresAt=$expiresAtEpochSeconds)"

    override fun equals(other: Any?): Boolean =
        other is PairingPayload && host == other.host && port == other.port && caSha256 == other.caSha256 &&
            code == other.code && expiresAtEpochSeconds == other.expiresAtEpochSeconds && pcName == other.pcName

    override fun hashCode(): Int = listOf(host, port, caSha256, code, expiresAtEpochSeconds, pcName).hashCode()

    companion object {
        const val PREFIX = "tatakai-link:v1?"
        const val OLD_PREFIX = "tatakai-link:v0"
        const val CODE_LENGTH = 10
        private const val CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
        private val IPV4 = Regex("""(0|[1-9]\d{0,2})\.(0|[1-9]\d{0,2})\.(0|[1-9]\d{0,2})\.(0|[1-9]\d{0,2})""")
        private val PORT = Regex("""[1-9]\d{0,4}""")
        private val HEX64 = Regex("""[0-9a-fA-F]{64}""")
        private val EPOCH = Regex("""\d{1,18}""")
        private val REQUIRED = listOf("h", "p", "ca", "c", "x")
        private val KNOWN = REQUIRED + "n"

        /** Parses [text] (surrounding whitespace allowed); `x` must be strictly after [nowEpochSeconds]. */
        fun parse(text: String, nowEpochSeconds: Long = System.currentTimeMillis() / 1000): PayloadResult {
            val trimmed = text.trim()
            if (trimmed.startsWith(OLD_PREFIX)) return invalid(PayloadError.OldVersion)
            if (!trimmed.startsWith(PREFIX)) return invalid(PayloadError.NotAPayload)
            val fields = mutableMapOf<String, String>()
            for (part in trimmed.removePrefix(PREFIX).split('&')) {
                val eq = part.indexOf('=')
                if (eq <= 0) return invalid(PayloadError.Malformed)
                val key = part.substring(0, eq)
                if (key !in KNOWN) return invalid(PayloadError.Malformed)
                if (fields.put(key, part.substring(eq + 1)) != null) return invalid(PayloadError.Malformed)
            }
            REQUIRED.firstOrNull { it !in fields }?.let { return invalid(PayloadError.Missing(it)) }

            val host = fields.getValue("h").takeIf(::isIpv4) ?: return invalid(PayloadError.InvalidHost)
            val port = fields.getValue("p").takeIf { PORT.matches(it) }?.toInt()?.takeIf { it in 1..65535 }
                ?: return invalid(PayloadError.InvalidPort)
            val ca = fields.getValue("ca").takeIf { HEX64.matches(it) }?.lowercase(Locale.ROOT)
                ?: return invalid(PayloadError.InvalidCa)
            val code = normalizeCode(fields.getValue("c")) ?: return invalid(PayloadError.InvalidCode)
            val expires = fields.getValue("x").takeIf { EPOCH.matches(it) }?.toLong()
                ?: return invalid(PayloadError.InvalidExpiry)
            if (expires <= nowEpochSeconds) return invalid(PayloadError.Expired)
            val name = fields["n"]?.let { runCatching { URLDecoder.decode(it, "UTF-8") }.getOrNull() }.orEmpty().take(64)
            return PayloadResult.Valid(PairingPayload(host, port, ca, code, expires, name))
        }

        /**
         * Crockford reading of the code: surrounding whitespace and hyphens ignored, lower case accepted, `O`->`0`, `I`/`L`->`1`.
         * Null unless exactly 10 symbols.
         */
        fun normalizeCode(input: String): String? {
            val out = StringBuilder(CODE_LENGTH)
            for (c in input.trim()) {
                val symbol = when (val upper = c.uppercaseChar()) {
                    '-' -> continue
                    'O' -> '0'
                    'I', 'L' -> '1'
                    else -> upper
                }
                if (symbol !in CROCKFORD || out.length == CODE_LENGTH) return null
                out.append(symbol)
            }
            return out.toString().takeIf { it.length == CODE_LENGTH }
        }

        private fun isIpv4(text: String): Boolean {
            val octets = IPV4.matchEntire(text)?.groupValues?.drop(1)?.map(String::toInt) ?: return false
            return octets.all { it in 0..255 } && octets.any { it != 0 }
        }

        private fun invalid(error: PayloadError) = PayloadResult.Invalid(error)
    }
}

sealed interface PayloadResult {
    data class Valid(val payload: PairingPayload) : PayloadResult
    data class Invalid(val error: PayloadError) : PayloadResult
}

/** Why a payload was rejected. Carries field names only, never values. */
sealed interface PayloadError {
    data object NotAPayload : PayloadError
    data object OldVersion : PayloadError
    data object Malformed : PayloadError
    data class Missing(val field: String) : PayloadError
    data object InvalidHost : PayloadError
    data object InvalidPort : PayloadError
    data object InvalidCa : PayloadError
    data object InvalidCode : PayloadError
    data object InvalidExpiry : PayloadError
    data object Expired : PayloadError
}
