// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.annotation.SuppressLint
import android.content.Context
import android.net.Uri
import android.os.Message
import android.view.View
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put
import java.io.ByteArrayInputStream
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicLong

/**
 * [BrowserRuntime] over hidden `android.webkit.WebView`s (docs/redesign/12 §3.7; Android counterpart of desktop `tk-app/src/live/browser.rs`).
 *
 * * One WebView per browser, created, driven and destroyed on the main thread; never attached to a window (always hidden: `headless: false`
 *   is only logged by [BrowserBridge]). It is laid out at a phone-sized viewport so pages that measure themselves behave.
 * * Every request (main frame and subresources) goes through [WebViewClient.shouldInterceptRequest]: only [BrowserPolicy.subresourceAllowed]
 *   URLs load (the provider domains + `challenges.cloudflare.com`), the rest gets an empty 403. Top-level navigations outside that set are
 *   cancelled, and `evaluate` re-checks the current page strictly with [BrowserPolicy.urlAllowed].
 * * Popups, downloads, file/content access, geolocation and permission requests are denied.
 * * Cookies come from the app-wide [CookieManager] (`Network.getCookies` shape). Storage is made ephemeral per browser on close: cookies,
 *   Web Storage and cache of the hosts it touched are deleted unless another open browser still uses that host.
 * * Each operation times out after `clamp(options.timeout, 1, 30)` s ([BrowserError.Timeout]); a browser closes itself
 *   [BrowserPolicy.AUTO_CLOSE_S] after creation; at most [BrowserPolicy.MAX_BROWSERS_PER_EXTENSION] per extension.
 */
class WebViewBrowserRuntime(
    context: Context,
    private val logs: ExtensionLogStore? = null,
) : BrowserRuntime {
    private val appContext = context.applicationContext
    private val main = Dispatchers.Main.immediate
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val sessions = ConcurrentHashMap<BrowserId, Session>()
    private val createLock = Mutex()
    private val seq = AtomicLong(1)

    private class Session(
        val id: BrowserId,
        val extId: String,
        val domains: List<String>,
        val timeoutMs: Long,
    ) {
        /** Main-thread state. */
        var view: WebView? = null
        var pendingLoad: CompletableDeferred<Unit>? = null
        var autoClose: Job? = null
        val touchedHosts: MutableSet<String> = ConcurrentHashMap.newKeySet()
    }

    override suspend fun newBrowser(extId: String, allowedDomains: List<String>, options: JsonObject): BrowserId {
        if (allowedDomains.isEmpty()) throw BrowserError.NoDomains()
        val timeoutMs = browserOperationTimeoutMs(options)
        val session = createLock.withLock {
            if (sessions.values.count { it.extId == extId } >= BrowserPolicy.MAX_BROWSERS_PER_EXTENSION) throw BrowserError.Limit()
            val id: BrowserId = "tk-browser-${seq.getAndIncrement()}"
            Session(id, extId, allowedDomains.toList(), timeoutMs).also { sessions[id] = it }
        }
        // Auto-close, armed before creation so a view created after a timed-out wait is still reclaimed.
        session.autoClose = scope.launch {
            delay(BrowserPolicy.AUTO_CLOSE_S * 1000L)
            if (sessions.containsKey(session.id)) {
                logs?.ok(extId, "browser", "browser closed automatically after ${BrowserPolicy.AUTO_CLOSE_S} s")
                close(session.id)
            }
        }
        try {
            within(timeoutMs) { withContext(main) { session.view = build(session) } }
        } catch (e: Throwable) {
            sessions.remove(session.id)
            session.autoClose?.cancel()
            scope.launch { withContext(main) { destroy(session) } }
            if (e is CancellationException && e !is TimeoutCancellationException) throw e
            logs?.err(extId, "browser", "the hidden browser could not be created: ${e.javaClass.simpleName}")
            throw if (e is BrowserError) e else BrowserError.Unavailable()
        }
        return session.id
    }

    override suspend fun navigate(id: BrowserId, url: String) {
        val s = sessions[id] ?: throw BrowserError.Closed()
        if (!BrowserPolicy.urlAllowed(url, s.domains)) throw BrowserError.UrlNotAllowed(url)
        val load = CompletableDeferred<Unit>()
        within(s.timeoutMs) {
            withContext(main) {
                val view = s.view ?: throw BrowserError.Closed()
                s.pendingLoad?.cancel()
                s.pendingLoad = load
                hostOf(url)?.let(s.touchedHosts::add)
                view.loadUrl(url)
            }
            load.await()
        }
    }

    override suspend fun evaluate(id: BrowserId, expression: String): JsonElement {
        val s = sessions[id] ?: throw BrowserError.Closed()
        if (expression.toByteArray(Charsets.UTF_8).size > BrowserPolicy.MAX_EXPRESSION_BYTES) throw BrowserError.Failed("Invalid ChromeDP expression")
        val result = CompletableDeferred<String?>()
        val raw = within(s.timeoutMs) {
            withContext(main) {
                val view = s.view ?: throw BrowserError.Closed()
                val current = view.url.orEmpty()
                if (!BrowserPolicy.urlAllowed(current, s.domains)) throw BrowserError.UrlNotAllowed(current)
                view.evaluateJavascript(wrapExpression(expression)) { value -> result.complete(value) }
            }
            result.await()
        }
        return parseEvaluation(raw)
    }

    override suspend fun cookies(id: BrowserId, urls: List<String>): List<JsonObject> {
        val s = sessions[id] ?: throw BrowserError.Closed()
        if (urls.isEmpty()) throw BrowserError.Failed("ChromeDP cookie URLs are required")
        urls.firstOrNull { !BrowserPolicy.urlAllowed(it, s.domains) }?.let { throw BrowserError.UrlNotAllowed(it) }
        return within(s.timeoutMs) {
            withContext(main) {
                if (s.view == null) throw BrowserError.Closed()
                val manager = CookieManager.getInstance()
                val seen = HashSet<String>()
                val out = ArrayList<JsonObject>()
                for (url in urls) {
                    val host = hostOf(url) ?: continue
                    s.touchedHosts += host
                    for ((name, value) in parseCookieHeader(manager.getCookie(url))) {
                        if (!seen.add("$host|$name")) continue
                        out += cookieJson(name, value, host)
                    }
                }
                out
            }
        }
    }

    override suspend fun close(id: BrowserId) {
        val s = sessions.remove(id) ?: return
        s.autoClose?.cancel()
        withContext(main) { destroy(s) }
    }

    // ---- main-thread helpers ----------------------------------------------------------------------------------------------------

    @SuppressLint("SetJavaScriptEnabled")
    private fun build(s: Session): WebView {
        val view = WebView(appContext)
        view.settings.apply {
            javaScriptEnabled = true // the challenge is a JS program
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setSupportMultipleWindows(false)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = true
            setGeolocationEnabled(false)
            cacheMode = WebSettings.LOAD_NO_CACHE
        }
        CookieManager.getInstance().apply {
            setAcceptCookie(true)
            setAcceptThirdPartyCookies(view, true) // the Turnstile iframe of challenges.cloudflare.com
        }
        view.webChromeClient = object : WebChromeClient() {
            override fun onPermissionRequest(request: PermissionRequest) = request.deny()

            override fun onCreateWindow(view: WebView?, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message?): Boolean = false

            override fun onGeolocationPermissionsShowPrompt(origin: String?, callback: GeolocationPermissions.Callback?) {
                callback?.invoke(origin, false, false)
            }
        }
        view.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView?, request: WebResourceRequest?): WebResourceResponse? {
                val url = request?.url?.toString() ?: return forbidden()
                return if (navigationAllowed(url, s.domains)) null else forbidden()
            }

            override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
                val url = request?.url?.toString() ?: return true
                val allowed = navigationAllowed(url, s.domains)
                if (allowed) hostOf(url)?.let(s.touchedHosts::add)
                return !allowed
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                url?.let(::hostOf)?.let(s.touchedHosts::add)
                s.pendingLoad?.let { if (!it.isCompleted) it.complete(Unit) }
            }

            override fun onReceivedError(view: WebView?, request: WebResourceRequest?, error: WebResourceError?) {
                if (request?.isForMainFrame == true) {
                    s.pendingLoad?.let { if (!it.isCompleted) it.completeExceptionally(BrowserError.Failed(error?.description?.toString() ?: "ChromeDP navigation failed")) }
                }
            }
        }
        view.setDownloadListener { _, _, _, _, _ -> } // downloads are ignored
        view.isFocusable = false
        view.measure(
            View.MeasureSpec.makeMeasureSpec(VIEWPORT_WIDTH, View.MeasureSpec.EXACTLY),
            View.MeasureSpec.makeMeasureSpec(VIEWPORT_HEIGHT, View.MeasureSpec.EXACTLY),
        )
        view.layout(0, 0, VIEWPORT_WIDTH, VIEWPORT_HEIGHT)
        view.onResume()
        return view
    }

    /** Destroys the view and wipes what it stored, keeping hosts another open browser still uses. Main thread; idempotent. */
    private fun destroy(s: Session) {
        s.pendingLoad?.let { if (!it.isCompleted) it.completeExceptionally(BrowserError.Closed()) }
        s.pendingLoad = null
        val view = s.view ?: return
        s.view = null
        runCatching {
            view.stopLoading()
            view.webChromeClient = null
            view.clearHistory()
            view.clearCache(true)
            view.onPause()
            view.removeAllViews()
            view.destroy()
        }
        val inUse = sessions.values.flatMap { it.touchedHosts + it.domains }.toSet()
        val hosts = (s.touchedHosts + s.domains).filter { it !in inUse }
        runCatching {
            val manager = CookieManager.getInstance()
            val storage = WebStorage.getInstance()
            for (host in hosts) {
                val url = "https://$host/"
                for ((name, _) in parseCookieHeader(manager.getCookie(url))) {
                    manager.setCookie(url, "$name=; Max-Age=0; Path=/")
                    manager.setCookie(url, "$name=; Max-Age=0; Path=/; Domain=$host")
                    manager.setCookie(url, "$name=; Max-Age=0; Path=/; Domain=.$host")
                }
                storage.deleteOrigin("https://$host")
            }
            manager.flush()
        }
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private suspend fun <T> within(timeoutMs: Long, block: suspend () -> T): T =
        try {
            withTimeout(timeoutMs) { block() }
        } catch (e: TimeoutCancellationException) {
            throw BrowserError.Timeout()
        }

    private fun forbidden(): WebResourceResponse =
        WebResourceResponse("text/plain", "utf-8", 403, "Forbidden", emptyMap(), ByteArrayInputStream(ByteArray(0)))

    private fun navigationAllowed(url: String, domains: List<String>): Boolean =
        url == "about:blank" || url == "about:srcdoc" || url.startsWith("data:") || url.startsWith("blob:") ||
            BrowserPolicy.subresourceAllowed(url, domains)

    private fun hostOf(url: String): String? = runCatching { Uri.parse(url).host?.lowercase() }.getOrNull()?.takeIf { it.isNotEmpty() }

    companion object {
        private const val VIEWPORT_WIDTH = 1080
        private const val VIEWPORT_HEIGHT = 1920

        /**
         * Wraps an expression (a trailing `;` is tolerated) so it returns `{ ok, v | e }` as a JSON string: an exception surfaces as an
         * error and `undefined` becomes `null`. Promises are not awaited (same as desktop).
         */
        internal fun wrapExpression(js: String): String {
            val expr = js.trim().trimEnd(';')
            return "(function(){try{var v=(\n$expr\n);return JSON.stringify({ok:true,v:v===undefined?null:v});}" +
                "catch(e){return JSON.stringify({ok:false,e:String(e&&e.message||e)});}})()"
        }

        /** `evaluateJavascript` hands back the JSON of the returned string: the envelope encoded twice. */
        internal fun parseEvaluation(raw: String?): JsonElement {
            if (raw != null && raw.length > BrowserPolicy.MAX_RESULT_BYTES * 2L) throw BrowserError.TooLarge()
            val bad = { BrowserError.Failed("ChromeDP evaluation failed") }
            val inner = runCatching { Json.parseToJsonElement(raw ?: "null") }.getOrNull() as? JsonPrimitive ?: throw bad()
            if (!inner.isString) throw bad()
            val env = runCatching { Json.parseToJsonElement(inner.content).jsonObject }.getOrNull() ?: throw bad()
            return if ((env["ok"] as? JsonPrimitive)?.content == "true") {
                env["v"] ?: JsonNull
            } else {
                throw BrowserError.Failed((env["e"] as? JsonPrimitive)?.content ?: "ChromeDP evaluation failed")
            }
        }

        /** `a=b; c=d` (what [CookieManager.getCookie] returns) -> pairs. */
        internal fun parseCookieHeader(header: String?): List<Pair<String, String>> =
            header.orEmpty().split(';').mapNotNull { part ->
                val p = part.trim()
                if (p.isEmpty()) return@mapNotNull null
                val eq = p.indexOf('=')
                if (eq <= 0) null else p.substring(0, eq).trim() to p.substring(eq + 1).trim()
            }

        /**
         * CDP `Network.Cookie` shape. [CookieManager] exposes only name and value, so the rest is what a session cookie of [host] looks like
         * (Toko only replays `name=value` with the user agent that solved the challenge).
         */
        internal fun cookieJson(name: String, value: String, host: String): JsonObject = buildJsonObject {
            put("name", name)
            put("value", value)
            put("domain", host)
            put("path", "/")
            put("expires", -1.0)
            put("size", name.length + value.length)
            put("httpOnly", false)
            put("secure", true)
            put("session", true)
            put("sameSite", "Lax")
        }
    }
}
