// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull

// Helpers behind BrowserPolicy (port of tk-ext browser.rs; docs/redesign/12 §3.7). Private to the extensions area (worker e).

/** ASCII-only lower-casing, like Rust `to_ascii_lowercase` (non-ASCII characters are kept as they are). */
internal fun String.asciiLower(): String {
    val sb = StringBuilder(length)
    for (c in this) sb.append(if (c in 'A'..'Z') c + 32 else c)
    return sb.toString()
}

internal object BrowserUrls {
    /** `[a-z0-9.-]+`, not an IPv4 literal, not localhost / `*.localhost` / `*.local` (input already lower-cased). */
    fun validDomain(d: String): Boolean =
        d.isNotEmpty() &&
            d.all { it in 'a'..'z' || it in '0'..'9' || it == '.' || it == '-' } &&
            !isIpv4(d) &&
            d != "localhost" &&
            !d.endsWith(".localhost") &&
            !d.endsWith(".local")

    /** Strict dotted quad as Rust's `Ipv4Addr::from_str` accepts it (four 0..=255 decimals, no leading zeros). */
    fun isIpv4(s: String): Boolean {
        val parts = s.split('.')
        if (parts.size != 4) return false
        return parts.all { p ->
            p.isNotEmpty() && p.length <= 3 && p.all { it in '0'..'9' } && (p.length == 1 || p[0] != '0') && p.toInt() <= 255
        }
    }

    /**
     * The lower-cased host of [url] when it is `https:` with a host, no user info and no explicit non-default port (the WHATWG parser of
     * Rust's `url` reports `:443` on https as no port); null otherwise. Hand-written instead of `java.net.URI` because URI rejects
     * characters browsers send unencoded (`|`, `{`, spaces in queries...), and only the authority matters here. Like WHATWG, tabs and
     * newlines are dropped, surrounding spaces trimmed and `\` ends the authority.
     */
    fun httpsHost(url: String): String? {
        val u = url.trim().filterNot { it == '\t' || it == '\n' || it == '\r' }
        val schemeEnd = u.indexOf(':')
        if (schemeEnd <= 0 || u.substring(0, schemeEnd).asciiLower() != "https") return null
        var rest = u.substring(schemeEnd + 1)
        // special scheme: any mix of '/' and '\' before the authority
        val slashes = rest.indexOfFirst { it != '/' && it != '\\' }.let { if (it < 0) rest.length else it }
        if (slashes == 0) return null
        rest = rest.substring(slashes)
        val end = rest.indexOfFirst { it == '/' || it == '\\' || it == '?' || it == '#' }.let { if (it < 0) rest.length else it }
        val authority = rest.substring(0, end)
        if (authority.isEmpty() || '@' in authority) return null
        val host: String
        val port: String?
        if (authority.startsWith("[")) {
            val close = authority.indexOf(']')
            if (close < 0) return null
            host = authority.substring(0, close + 1)
            val after = authority.substring(close + 1)
            port = if (after.isEmpty()) null else if (after.startsWith(":")) after.substring(1) else return null
        } else {
            val colon = authority.indexOf(':')
            host = if (colon < 0) authority else authority.substring(0, colon)
            port = if (colon < 0) null else authority.substring(colon + 1)
        }
        if (port != null && port.isNotEmpty()) {
            if (!port.all { it in '0'..'9' }) return null
            val n = port.trimStart('0').ifEmpty { "0" }
            if (n.length > 5 || n.toInt() > 65535) return null
            if (n != "443") return null
        }
        val h = host.asciiLower()
        return h.ifEmpty { null }
    }
}

/**
 * Per-operation timeout of a browser in ms. Legacy: `Math.min(30, Math.max(1, Number(options.timeout) || 20))` seconds;
 * [options] is the bundle's `newBrowser` argument (`timeout` as a number or a numeric string).
 */
fun browserOperationTimeoutMs(options: JsonObject): Long {
    val t = when (val v = options["timeout"]) {
        null, is JsonNull -> null
        is JsonPrimitive -> if (v.isString) v.contentOrNull?.trim()?.toDoubleOrNull() else v.doubleOrNull
        else -> null
    }
    val secs = t?.takeIf { it.isFinite() && it != 0.0 } ?: BrowserPolicy.DEFAULT_TIMEOUT_S.toDouble()
    return (secs.coerceIn(1.0, BrowserPolicy.MAX_TIMEOUT_S.toDouble()) * 1000).toLong()
}

/** `options.headless === false` (a debugging window on desktop; ignored on Android, where the browser is always hidden). */
fun browserWantsWindow(options: JsonObject): Boolean = (options["headless"] as? JsonPrimitive)?.let { !it.isString && it.content == "false" } == true
