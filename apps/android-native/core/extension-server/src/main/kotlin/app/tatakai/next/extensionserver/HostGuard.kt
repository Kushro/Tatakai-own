// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.net.URI
import java.util.Locale

/**
 * Transport rule of the extension server (docs/redesign/13 §3.8, same rule as `src/core/extensions/server/hostGuard.ts`):
 * `https:` is allowed for any host; `http:` only for a literal local host (no DNS is ever resolved here):
 * loopback (127.0.0.0/8, ::1, `localhost`, `*.localhost`), RFC1918 (10/8, 172.16/12, 192.168/16), link-local (169.254/16, fe80::/10),
 * CGNAT / Tailscale (100.64/10), IPv6 ULA (fc00::/7) and mDNS `*.local` names. Any other scheme, a dotless name or a public host over
 * `http:` is blocked. URLs carrying credentials (`user:pass@`) are blocked too.
 *
 * The app's network_security_config permits cleartext globally so that this guard is the real policy for the extension server; it runs
 * on connect, on pairing and before every request, including the media URLs the server returns.
 */
object HostGuard {
    fun isAllowed(url: String): Boolean {
        val uri = try {
            URI(url.trim())
        } catch (_: Exception) {
            return false
        }
        val scheme = uri.scheme?.lowercase(Locale.ROOT) ?: return false
        if (uri.rawUserInfo != null) return false
        val host = hostOf(uri) ?: return false
        return when (scheme) {
            "https" -> true
            "http" -> isLocalHost(host)
            else -> false
        }
    }

    /** Throws [ExtensionServerException] `insecure_host` when [url] is not allowed. */
    fun require(url: String) {
        if (!isAllowed(url)) throw ExtensionServerException(ExtensionServerException.INSECURE_HOST)
    }

    /** Host of [uri] lower-cased, IPv6 without brackets, without a trailing dot; null when the URI has no usable host. */
    internal fun hostOf(uri: URI): String? {
        val raw = uri.host ?: return null
        val host = raw.removePrefix("[").removeSuffix("]").trimEnd('.').lowercase(Locale.ROOT)
        return host.takeIf { it.isNotEmpty() }
    }

    /** Whether [host] (already normalized by [hostOf]) is a local address in the sense of the guard. */
    fun isLocalHost(host: String): Boolean {
        val h = host.removePrefix("[").removeSuffix("]").trimEnd('.').lowercase(Locale.ROOT)
        if (h.isEmpty()) return false
        if (h == "localhost" || h.endsWith(".localhost")) return true
        if (h.endsWith(".local") && h.length > ".local".length) return true
        ipv4(h)?.let { return isLocalIpv4(it) }
        if (':' in h) {
            val bytes = ipv6(h) ?: return false
            return isLocalIpv6(bytes)
        }
        return false
    }

    private val IPV4 = Regex("""(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})""")
    private val IPV6_CHARS = Regex("""[0-9a-f:.]+""")

    /** Strict dotted quad; null for anything else (no octal / hex / short forms). */
    internal fun ipv4(text: String): IntArray? {
        val m = IPV4.matchEntire(text) ?: return null
        val octets = m.groupValues.drop(1).map { part ->
            if (part.length > 1 && part.startsWith('0')) return null
            part.toInt().takeIf { it in 0..255 } ?: return null
        }
        return octets.toIntArray()
    }

    private fun isLocalIpv4(o: IntArray): Boolean {
        val a = o[0]
        val b = o[1]
        return a == 127 ||
            a == 10 ||
            (a == 172 && b in 16..31) ||
            (a == 192 && b == 168) ||
            (a == 169 && b == 254) ||
            (a == 100 && b in 64..127)
    }

    /**
     * IPv6 literal -> 16 bytes. Only text made of hex digits, `:` and `.` is handed to [InetAddress.getByName], which parses a literal
     * containing `:` without any lookup. IPv4-mapped addresses come back as the IPv4 bytes (length 4).
     */
    private fun ipv6(text: String): ByteArray? {
        if (!IPV6_CHARS.matches(text) || text.count { it == ':' } < 2) return null
        return try {
            when (val addr = InetAddress.getByName(text)) {
                is Inet6Address -> addr.address
                is Inet4Address -> addr.address
                else -> null
            }
        } catch (_: Exception) {
            null
        }
    }

    private fun isLocalIpv6(b: ByteArray): Boolean {
        if (b.size == 4) return isLocalIpv4(IntArray(4) { b[it].toInt() and 0xff })
        if (b.size != 16) return false
        val first = b[0].toInt() and 0xff
        val second = b[1].toInt() and 0xff
        val loopback = (0 until 15).all { b[it].toInt() == 0 } && b[15].toInt() == 1
        return loopback ||
            (first == 0xfe && (second and 0xc0) == 0x80) || // fe80::/10
            (first and 0xfe) == 0xfc // fc00::/7 (Tailscale fd7a:115c:a1e0::/48)
    }

    /**
     * Normalizes a server base URL typed or scanned by the user to its origin (`scheme://host[:port]`, no trailing slash). A single
     * trailing `/` is accepted; a path, query, fragment or credentials are not. Null when invalid; the guard is applied separately.
     */
    fun originOf(url: String): String? {
        val uri = try {
            URI(url.trim())
        } catch (_: Exception) {
            return null
        }
        val scheme = uri.scheme?.lowercase(Locale.ROOT)?.takeIf { it == "http" || it == "https" } ?: return null
        if (uri.rawUserInfo != null || uri.rawQuery != null || uri.rawFragment != null) return null
        if (!(uri.rawPath.isNullOrEmpty() || uri.rawPath == "/")) return null
        val rawHost = uri.host ?: return null
        val host = rawHost.lowercase(Locale.ROOT)
        val port = uri.port
        if (port == 0 || port > 65535) return null
        val defaultPort = (scheme == "http" && port == 80) || (scheme == "https" && port == 443)
        return buildString {
            append(scheme).append("://").append(host)
            if (port > 0 && !defaultPort) append(':').append(port)
        }
    }

    /** Whether [url] lives on the same origin as [origin] (scheme, host and effective port). */
    fun sameOrigin(url: String, origin: String): Boolean {
        val a = try { URI(url.trim()) } catch (_: Exception) { return false }
        val b = try { URI(origin.trim()) } catch (_: Exception) { return false }
        val sa = a.scheme?.lowercase(Locale.ROOT) ?: return false
        val sb = b.scheme?.lowercase(Locale.ROOT) ?: return false
        if (sa != sb || a.rawUserInfo != null) return false
        val ha = hostOf(a) ?: return false
        val hb = hostOf(b) ?: return false
        return ha == hb && effectivePort(a, sa) == effectivePort(b, sb)
    }

    private fun effectivePort(u: URI, scheme: String): Int = when {
        u.port > 0 -> u.port
        scheme == "https" -> 443
        else -> 80
    }
}
