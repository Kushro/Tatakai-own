// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.annotation.SuppressLint
import android.content.Context
import android.os.Message
import android.util.Log
import android.view.View
import android.webkit.CookieManager
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import app.tatakai.next.model.EMBED_SNIFF_TIMEOUT_MS
import app.tatakai.next.model.EmbedCapture
import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import java.io.ByteArrayInputStream

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.3, worker t; checklist sources.embed-sniffer,
 * sources.embed-sniffer.adblock): [EmbedSniffer] over a hidden `android.webkit.WebView` (same construction as [WebViewBrowserRuntime]:
 * main thread, never attached to a window, phone-sized layout, popups / downloads / permissions denied).
 *
 * The page is loaded with the extension's headers; every request goes through `shouldInterceptRequest`: ad / popunder hosts
 * (`assets/adblock-hosts.txt`, [EmbedDetector.isBlockedHost]) get an empty answer, and the first request [EmbedDetector.mediaContainer]
 * recognises as HLS / MP4 completes the capture with its request headers (+ the `Cookie` of [CookieManager] and the WebView user agent).
 * 2 s after `onPageFinished` the page gets a nudge (`video.play()` and a click on the first play button) because many players only ask
 * for the manifest after a click. Top-level navigations outside the embed's site are cancelled ([EmbedDetector.navigationAllowed]);
 * `onCreateWindow` refuses every popup. After `timeoutMs` -> [EmbedSniffException.TIMEOUT]; main-frame error -> [EmbedSniffException.BLOCKED].
 * The WebView is destroyed in every exit path; at most [MAX_CONCURRENT] sniffs run at once (the third waits). One log line per sniff in
 * [ExtensionLogStore] under op `embed` and in logcat (`Tatakai`), host only (never the full URL with its query).
 */
class WebViewEmbedSniffer(
    context: Context,
    private val logs: ExtensionLogStore? = null,
) : EmbedSniffer {
    private val appContext = context.applicationContext
    private val main = Dispatchers.Main.immediate
    private val permits = Semaphore(MAX_CONCURRENT)

    /** Ad hosts, read once from the bundled asset (empty when the asset is missing: nothing is blocked then). */
    private val blockedHosts: Set<String> by lazy {
        runCatching { appContext.assets.open(ADBLOCK_ASSET).bufferedReader().use { EmbedDetector.parseHostList(it.readText()) } }
            .getOrDefault(emptySet())
    }

    override suspend fun sniff(embedUrl: String, headers: Map<String, String>, extId: String, timeoutMs: Long): EmbedCapture {
        val host = EmbedDetector.hostOf(embedUrl) ?: throw EmbedSniffException(EmbedSniffException.BLOCKED)
        if (EmbedDetector.isBlockedHost(host, blockedHosts)) {
            note(extId, false, "embed host $host is on the ad block list")
            throw EmbedSniffException(EmbedSniffException.BLOCKED)
        }
        val budget = timeoutMs.coerceIn(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
        return permits.withPermit {
            val started = System.currentTimeMillis()
            val result = CompletableDeferred<EmbedCapture>()
            var view: WebView? = null
            try {
                withTimeout(budget) {
                    coroutineScope {
                        withContext(main) { view = build(embedUrl, headers, result) }
                        val capture = result.await()
                        note(extId, true, "embed $host -> ${capture.container.name} from ${EmbedDetector.hostOf(capture.url) ?: "?"} in ${System.currentTimeMillis() - started} ms")
                        capture
                    }
                }
            } catch (e: TimeoutCancellationException) {
                note(extId, false, "embed $host: no media request within ${budget / 1000} s")
                throw EmbedSniffException(EmbedSniffException.TIMEOUT, e)
            } catch (e: EmbedSniffException) {
                note(extId, false, "embed $host failed: ${e.reason}")
                throw e
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                note(extId, false, "embed $host: the hidden browser is unavailable (${e.javaClass.simpleName})")
                throw EmbedSniffException(EmbedSniffException.UNAVAILABLE, e)
            } finally {
                val v = view
                if (v != null) withContext(NonCancellable + main) { destroy(v) }
            }
        }
    }

    // ---- main thread ------------------------------------------------------------------------------------------------------------

    @SuppressLint("SetJavaScriptEnabled")
    private fun build(embedUrl: String, headers: Map<String, String>, result: CompletableDeferred<EmbedCapture>): WebView {
        val view = WebView(appContext)
        val extraUa = headers.entries.firstOrNull { it.key.equals("User-Agent", ignoreCase = true) }?.value
        view.settings.apply {
            javaScriptEnabled = true // players are JS programs
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            setSupportMultipleWindows(false)
            javaScriptCanOpenWindowsAutomatically = false
            mediaPlaybackRequiresUserGesture = false // the nudge may start the player without a real gesture
            setGeolocationEnabled(false)
            cacheMode = WebSettings.LOAD_NO_CACHE
            if (!extraUa.isNullOrBlank()) userAgentString = extraUa
        }
        val userAgent = view.settings.userAgentString
        CookieManager.getInstance().apply {
            setAcceptCookie(true)
            setAcceptThirdPartyCookies(view, true)
        }
        view.webChromeClient = object : WebChromeClient() {
            override fun onPermissionRequest(request: PermissionRequest) = request.deny()

            // sources.embed-sniffer.adblock: no popup, no new tab, ever.
            override fun onCreateWindow(view: WebView?, isDialog: Boolean, isUserGesture: Boolean, resultMsg: Message?): Boolean = false

            override fun onGeolocationPermissionsShowPrompt(origin: String?, callback: GeolocationPermissions.Callback?) {
                callback?.invoke(origin, false, false)
            }
        }
        view.webViewClient = object : WebViewClient() {
            override fun shouldInterceptRequest(view: WebView?, request: WebResourceRequest?): WebResourceResponse? {
                val req = request ?: return empty()
                val url = req.url?.toString() ?: return empty()
                val reqHost = EmbedDetector.hostOf(url)
                if (reqHost != null && EmbedDetector.isBlockedHost(reqHost, blockedHosts)) return empty()
                if (!result.isCompleted && !EmbedDetector.isIgnorable(url)) {
                    val accept = req.requestHeaders?.entries?.firstOrNull { it.key.equals("Accept", ignoreCase = true) }?.value
                    val container = EmbedDetector.mediaContainer(url, accept?.takeIf { it.contains("mpegurl", ignoreCase = true) })
                    if (container != null) {
                        val cookie = runCatching { CookieManager.getInstance().getCookie(url) }.getOrNull()
                        val replay = EmbedDetector.replayHeaders(req.requestHeaders.orEmpty(), cookie, userAgent).toMutableMap()
                        if (replay.keys.none { it.equals("Referer", ignoreCase = true) }) replay["Referer"] = embedUrl
                        result.complete(EmbedCapture(url, replay, container))
                    }
                }
                return null
            }

            override fun shouldOverrideUrlLoading(view: WebView?, request: WebResourceRequest?): Boolean {
                val req = request ?: return true
                val url = req.url?.toString() ?: return true
                if (req.isForMainFrame && !EmbedDetector.navigationAllowed(embedUrl, url)) return true
                val h = EmbedDetector.hostOf(url)
                return h != null && EmbedDetector.isBlockedHost(h, blockedHosts)
            }

            override fun onPageFinished(view: WebView?, url: String?) {
                val v = view ?: return
                v.postDelayed({ if (!result.isCompleted) runCatching { v.evaluateJavascript(NUDGE_JS, null) } }, NUDGE_DELAY_MS)
            }

            override fun onReceivedError(view: WebView?, request: WebResourceRequest?, error: WebResourceError?) {
                if (request?.isForMainFrame == true && !result.isCompleted) {
                    result.completeExceptionally(EmbedSniffException(EmbedSniffException.BLOCKED))
                }
            }

            override fun onReceivedHttpError(view: WebView?, request: WebResourceRequest?, errorResponse: WebResourceResponse?) {
                val code = errorResponse?.statusCode ?: return
                if (request?.isForMainFrame == true && code >= 400 && !result.isCompleted) {
                    result.completeExceptionally(EmbedSniffException(if (code == 404 || code == 410) EmbedSniffException.NO_MEDIA else EmbedSniffException.BLOCKED))
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
        val loadHeaders = headers.filterKeys { !it.equals("User-Agent", ignoreCase = true) }
        view.loadUrl(embedUrl, loadHeaders)
        return view
    }

    /** Main thread; idempotent enough (a destroyed view ignores further calls). */
    private fun destroy(view: WebView) {
        runCatching {
            view.stopLoading()
            view.webChromeClient = null
            view.loadUrl("about:blank")
            view.clearHistory()
            view.clearCache(true)
            view.onPause()
            view.removeAllViews()
            view.destroy()
        }
    }

    private fun empty(): WebResourceResponse =
        WebResourceResponse("text/plain", "utf-8", 204, "No Content", emptyMap(), ByteArrayInputStream(ByteArray(0)))

    private fun note(extId: String, ok: Boolean, message: String) {
        if (ok) logs?.ok(extId, "embed", message) else logs?.warn(extId, "embed", message)
        Log.i(LOG_TAG, "[$extId] $message")
    }

    companion object {
        const val MAX_CONCURRENT: Int = 2
        val DEFAULT_TIMEOUT_MS: Long = EMBED_SNIFF_TIMEOUT_MS
        const val ADBLOCK_ASSET = "adblock-hosts.txt"
        private const val MIN_TIMEOUT_MS = 5_000L
        private const val MAX_TIMEOUT_MS = 60_000L
        private const val NUDGE_DELAY_MS = 2_000L
        private const val VIEWPORT_WIDTH = 1080
        private const val VIEWPORT_HEIGHT = 1920
        private const val LOG_TAG = "Tatakai"

        /** Starts the first `<video>` and clicks the first thing that looks like a play button (also inside same-origin iframes). */
        internal const val NUDGE_JS: String = """(function(){try{
var docs=[document];document.querySelectorAll('iframe').forEach(function(f){try{if(f.contentDocument)docs.push(f.contentDocument);}catch(e){}});
docs.forEach(function(d){
 d.querySelectorAll('video').forEach(function(v){try{v.muted=true;var p=v.play();if(p&&p.catch)p.catch(function(){});}catch(e){}});
 var b=d.querySelector('.jw-icon-display,.vjs-big-play-button,.plyr__control--overlaid,[aria-label*="Play" i],button[class*="play" i],div[class*="play" i]');
 if(b){try{b.click();}catch(e){}}
});}catch(e){}})();"""
    }
}
