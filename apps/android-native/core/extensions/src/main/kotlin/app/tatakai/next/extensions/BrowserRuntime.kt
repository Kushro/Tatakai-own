// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

// Kotlin mirror of apps/desktop-native/crates/tk-ext/src/browser.rs (docs/redesign/09 §5, 12 §3.7): a hidden browser for extensions that
// need one (Cloudflare bypass of Toko / Latino, see Extensiones Tatakai-own-linux/Toko/src/utils/common/cf-bypass.ts). Signatures FINAL.
// Implementation: worker e, `WebViewBrowserRuntime` in :core:extensions-quickjs (one hidden android.webkit.WebView per browser, main thread,
// CookieManager for Network.getCookies). The JS bridge contract (global names + method set) is in 12 §3.7.

/** Opaque id of one browser session. */
typealias BrowserId = String

/** Messages are identical to desktop `BrowserError` (bundles may match on them). */
sealed class BrowserError(message: String) : Exception(message) {
    class NoDomains : BrowserError("ChromeDP requires a permitted public provider website")
    class Limit : BrowserError("ChromeDP browser limit reached")
    class UrlNotAllowed(val url: String) : BrowserError("ChromeDP URL is outside the provider website")
    class Closed : BrowserError("ChromeDP browser is closed")
    class Timeout : BrowserError("ChromeDP operation timed out")
    class TooLarge : BrowserError("ChromeDP response is too large")
    class Unavailable : BrowserError("the browser runtime is unavailable")
    class Failed(message: String) : BrowserError(message)
}

/**
 * Injected into `ExtensionHost` / `QuickJsEngine` (worker e decides how) and handed to a bundle ONLY when its instance has
 * `configureBrowserRuntime` AND [BrowserPolicy.browserDomains] of its manifest is non-empty. All methods may be called from any coroutine;
 * the implementation hops to the main thread for WebView work. Throws [BrowserError].
 */
interface BrowserRuntime {
    /** Open a hidden browser for [extId] restricted to [allowedDomains] (never empty). [options] = the bundle's `{ headless, timeout }`. */
    suspend fun newBrowser(extId: String, allowedDomains: List<String>, options: JsonObject): BrowserId

    /** Load [url] and return once the page finished loading (or the operation timeout hits). */
    suspend fun navigate(id: BrowserId, url: String)

    /** Evaluate [expression] in the current page; its JSON value (JsonNull for undefined). */
    suspend fun evaluate(id: BrowserId, expression: String): JsonElement

    /** Cookies visible to [urls], CDP `Network.Cookie` shape `{ name, value, domain, path, expires, httpOnly, secure, session, sameSite }`. */
    suspend fun cookies(id: BrowserId, urls: List<String>): List<JsonObject>

    /** Close and forget the browser (idempotent). */
    suspend fun close(id: BrowserId)
}

/** Limits and URL rules of desktop browser.rs (legacy `checkUrl`). Bodies: worker e (1:1 port of `browser_domains` / `browser_url_allowed`). */
object BrowserPolicy {
    const val MAX_BROWSERS_PER_EXTENSION: Int = 2
    const val DEFAULT_TIMEOUT_S: Int = 20
    const val MAX_TIMEOUT_S: Int = 30
    const val MAX_EXPRESSION_BYTES: Int = 32 * 1024
    const val MAX_RESULT_BYTES: Int = 2_000_000
    const val AUTO_CLOSE_S: Int = 150
    const val MAX_COOKIE_URLS: Int = 10
    const val MAX_SLEEP_MS: Int = 10_000

    /** A host call that opened a browser may run this long (a Cloudflare solve takes 15-45 s), instead of JsEngine.CALL_TIMEOUT_MS. */
    const val CALL_BUDGET_WITH_BROWSER_MS: Long = 90_000

    /** Extra subresource host a challenge page may load. */
    const val CHALLENGE_HOST: String = "challenges.cloudflare.com"

    /** manifest.browserDomains covered by a `network:domain:<d>` permission (equal or subdomain), valid, not IP/localhost/.local, lowercase, deduped. */
    fun browserDomains(manifest: ExtensionManifest): List<String> {
        val network = manifest.permissions
            .filter { it.startsWith(NetworkPolicy.PREFIX) }
            .map { it.removePrefix(NetworkPolicy.PREFIX).trim().removePrefix("*.").asciiLower() }
            .filter { it.isNotEmpty() }
        val out = ArrayList<String>()
        for (raw in manifest.browserDomains) {
            val d = raw.trim().asciiLower()
            if (!BrowserUrls.validDomain(d) || d in out) continue
            if (network.any { n -> d == n || d.endsWith(".$n") }) out += d
        }
        return out
    }

    /** `https:`, no user info, no explicit port, host equal to or a subdomain of one of [domains]. */
    fun urlAllowed(url: String, domains: List<String>): Boolean {
        val host = BrowserUrls.httpsHost(url) ?: return false
        return domains.any { d -> host == d || host.endsWith(".$d") }
    }

    /** [urlAllowed] or a URL of [CHALLENGE_HOST]. */
    fun subresourceAllowed(url: String, domains: List<String>): Boolean =
        urlAllowed(url, domains) || urlAllowed(url, listOf(CHALLENGE_HOST))
}
