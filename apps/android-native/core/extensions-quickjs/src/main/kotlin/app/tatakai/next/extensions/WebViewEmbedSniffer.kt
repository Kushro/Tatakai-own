// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.content.Context
import app.tatakai.next.model.EMBED_SNIFF_TIMEOUT_MS
import app.tatakai.next.model.EmbedCapture
import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.3, worker t, checklist sources.embed-sniffer): [EmbedSniffer] over
 * a hidden `android.webkit.WebView` (same construction as [WebViewBrowserRuntime]: main thread, never attached to a window, phone-sized
 * layout, popups / downloads / permissions denied).
 *
 * Plan for t: load [embedUrl] with the extension's headers (`loadUrl(url, headers)`), intercept every request in
 * `WebViewClient.shouldInterceptRequest`; the first request whose path ends in `.m3u8` / `.mp4` (or whose `Accept` / known CDN pattern
 * says HLS / MP4) completes a `CompletableDeferred<EmbedCapture>` with the request URL + `request.requestHeaders` (+ `Cookie` from
 * `CookieManager`); the page keeps loading until then (the embed's own player usually requests the manifest after a click: r injects a
 * `video.play()` / first-button click through `evaluateJavascript` 2 s after `onPageFinished`). After [timeoutMs] -> [EmbedSniffException.TIMEOUT];
 * page error -> [EmbedSniffException.BLOCKED]. The WebView is destroyed in every exit path; at most [MAX_CONCURRENT] sniffs (third waits).
 * Logs one line per sniff to [ExtensionLogStore] under op `embed` (URL host only, never the full URL with query).
 */
class WebViewEmbedSniffer(
    context: Context,
    private val logs: ExtensionLogStore? = null,
) : EmbedSniffer {
    private val appContext = context.applicationContext

    override suspend fun sniff(embedUrl: String, headers: Map<String, String>, extId: String, timeoutMs: Long): EmbedCapture {
        // wave B (t): implement as described above. Until then every embed source fails fast and the flow moves to the next source.
        logs?.warn(extId, "embed", "embed sniffer not implemented yet")
        throw EmbedSniffException(EmbedSniffException.UNAVAILABLE)
    }

    companion object {
        const val MAX_CONCURRENT: Int = 2
        val DEFAULT_TIMEOUT_MS: Long = EMBED_SNIFF_TIMEOUT_MS
    }
}
