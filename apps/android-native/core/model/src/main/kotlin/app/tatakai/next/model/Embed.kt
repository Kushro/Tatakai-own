// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.3, plan 14 §0.1 #13): embed-source sniffer in the hidden
// WebView. An extension source of type `embed` (an iframe page, not a media URL) is opened in a hidden WebView; the first m3u8 / mp4
// request the page makes is captured with its headers and played by the normal engine. Desktop counterpart: tk-app/src/live/browser.rs
// (worker k). Signatures FINAL; body: worker t (`WebViewEmbedSniffer` in :core:extensions-quickjs, bound in `extensionsModule`).

/** Display string of `SourceCandidate.container` for an embed page (never parses to a [Container]: it is not playable as is). */
const val EMBED_CONTAINER_LABEL: String = "EMBED"

/** How long the sniffer waits for the first media request before failing the source (checklist `sources.embed-sniffer`: 20 s). */
const val EMBED_SNIFF_TIMEOUT_MS: Long = 20_000

/** The media request captured from the embed page. [headers] = the request headers the page sent (Referer, Origin, cookies, UA). */
data class EmbedCapture(val url: String, val headers: Map<String, String>, val container: Container) {
    /** The playable stream, keeping the candidate's identity (`provider` / `extensionId`) so reliability stats stay per source. */
    fun toStream(candidate: SourceCandidate): StreamSource = StreamSource(
        url = url,
        headers = headers,
        container = container,
        quality = candidate.quality.label().takeIf { it.isNotEmpty() },
        provider = candidate.label,
        extensionId = candidate.extId,
        subtitles = candidate.web?.subtitles.orEmpty(),
    )
}

/** Why a sniff failed; the player shows it as the source's fail detail and moves to the next source. */
class EmbedSniffException(val reason: String, cause: Throwable? = null) : Exception("embed sniff: $reason", cause) {
    companion object {
        const val TIMEOUT = "timeout"
        const val BLOCKED = "blocked"
        const val UNAVAILABLE = "unavailable"
        const val NO_MEDIA = "no_media"
    }
}

/**
 * Opens [embedUrl] in a hidden WebView (headers = the extension's, so Referer / cookies match), intercepts every request and resolves
 * with the first one whose URL or content type is HLS (`.m3u8`, `application/vnd.apple.mpegurl`) or MP4 (`.mp4`, `video/mp4`); DASH
 * is ignored (not playable in M0). Throws [EmbedSniffException] after [timeoutMs] or when the page cannot load. One WebView per call,
 * destroyed on completion; at most 2 concurrent sniffs.
 */
interface EmbedSniffer {
    suspend fun sniff(embedUrl: String, headers: Map<String, String>, extId: String, timeoutMs: Long = EMBED_SNIFF_TIMEOUT_MS): EmbedCapture
}
