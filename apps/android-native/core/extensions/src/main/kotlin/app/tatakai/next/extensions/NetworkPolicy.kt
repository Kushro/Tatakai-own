// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import java.net.InetAddress
import java.net.URI
import java.util.Locale

/**
 * `network:domain:<host>` permissions of a manifest. A request host matches when it is exactly the host or ends with
 * `.<host>`; `*` allows any host (still subject to [SsrfGuard]). contracts/extension-host/routes.v1.json.
 */
class NetworkPolicy private constructor(private val hosts: Set<String>) {
    val allowsAnyHost: Boolean get() = "*" in hosts

    fun allowsHost(host: String): Boolean {
        val h = host.lowercase(Locale.ROOT).trimEnd('.')
        if (h.isEmpty()) return false
        if (allowsAnyHost) return true
        return hosts.any { allowed -> h == allowed || h.endsWith(".$allowed") }
    }

    /**
     * Validates [url] against the permission set. Only http/https with a host is allowed; credentials in the URL are refused.
     * @throws ExtError.PermissionDenied (fixed message "Network permission denied")
     */
    fun requireAllowed(url: String): URI {
        val uri = parseHttpUri(url) ?: throw ExtError.PermissionDenied()
        val host = uri.host ?: throw ExtError.PermissionDenied()
        if (!allowsHost(host)) throw ExtError.PermissionDenied()
        return uri
    }

    companion object {
        const val PREFIX = "network:domain:"

        fun fromPermissions(permissions: List<String>): NetworkPolicy =
            NetworkPolicy(
                permissions.filter { it.startsWith(PREFIX) }
                    .map { it.removePrefix(PREFIX).trim().lowercase(Locale.ROOT).trimEnd('.') }
                    .filter { it.isNotEmpty() }
                    .toSet(),
            )

        val NONE: NetworkPolicy = NetworkPolicy(emptySet())

        /** Null unless [url] is an absolute http(s) URL with a host and without user-info. */
        fun parseHttpUri(url: String): URI? {
            val uri = try {
                URI(url)
            } catch (_: Exception) {
                return null
            }
            val scheme = uri.scheme?.lowercase(Locale.ROOT)
            if (scheme != "http" && scheme != "https") return null
            if (uri.host.isNullOrEmpty() || uri.userInfo != null) return null
            return uri
        }
    }
}

/** SSRF policy of the native fetch (globals.v1.md section 2): no loopback / private / link-local / CGNAT / ULA targets. */
object SsrfGuard {
    fun isForbidden(address: InetAddress): Boolean {
        if (address.isAnyLocalAddress || address.isLoopbackAddress || address.isLinkLocalAddress ||
            address.isSiteLocalAddress || address.isMulticastAddress
        ) {
            return true
        }
        val b = address.address
        if (b.size == 4) {
            val first = b[0].toInt() and 0xff
            val second = b[1].toInt() and 0xff
            if (first == 0) return true // 0.0.0.0/8
            if (first == 100 && second in 64..127) return true // CGNAT 100.64.0.0/10
            if (first >= 240) return true // reserved + broadcast
        } else if (b.size == 16) {
            if ((b[0].toInt() and 0xfe) == 0xfc) return true // ULA fc00::/7
        }
        return false
    }

    /** Cheap pre-DNS check for hostnames that are obviously local. */
    fun isForbiddenHostName(host: String): Boolean {
        val h = host.lowercase(Locale.ROOT).trimEnd('.')
        return h == "localhost" || h.endsWith(".localhost") || h.endsWith(".local") || h.endsWith(".internal")
    }
}
