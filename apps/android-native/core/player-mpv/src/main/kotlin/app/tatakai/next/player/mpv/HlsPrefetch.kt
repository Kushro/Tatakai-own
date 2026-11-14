// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import app.tatakai.next.model.Container
import app.tatakai.next.player.HlsMasterFilter
import app.tatakai.next.player.PlayableSource
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * BUG-059: before libmpv opens an HLS master, download it and, when [HlsMasterFilter] drops variants / renditions, write the filtered
 * master to the cache and play that file instead. FFmpeg then opens only the variant and the languages it will play, so a rendition
 * without segments yet (anizone through the PC's extension server: 15 audio renditions) no longer keeps it at 0:00.
 *
 * Only for sources that need no HTTP headers (the PC's extension-server proxy URLs carry their own): FFmpeg's nested requests from a
 * local master do not inherit mpv's User-Agent / Referer / header fields, so a CDN that checks them would refuse the renditions. Those
 * sources keep the original master (and the start safety net of [MpvPlayerSession]).
 */
internal object HlsPrefetch {
    private const val MAX_MASTER_BYTES = 512L * 1024
    private const val DIR = "mpv-hls"

    private val client: OkHttpClient by lazy {
        OkHttpClient.Builder().callTimeout(FETCH_TIMEOUT_S, TimeUnit.SECONDS).followRedirects(true).build()
    }

    fun applies(source: PlayableSource): Boolean =
        source.container == Container.HLS && !source.torrent && source.headers.isEmpty() && source.httpClient == null &&
            (source.url.startsWith("http://") || source.url.startsWith("https://"))

    /** Path of the filtered master in the cache, or null to play [url] as it is (not a master, nothing to drop, or any failure). */
    fun filteredMaster(context: Context, url: String, audioLang: String?, subLang: String?): String? = runCatching {
        client.newCall(Request.Builder().url(url).build()).execute().use { resp ->
            if (!resp.isSuccessful) return null
            val body = resp.body
            if (body.contentLength() > MAX_MASTER_BYTES) return null
            val source = body.source()
            if (source.request(MAX_MASTER_BYTES + 1)) return null
            val text = source.buffer.readUtf8()
            val filtered = HlsMasterFilter.filter(text, resp.request.url.toString(), audioLang, subLang) ?: return null
            val dir = File(context.cacheDir, DIR).apply { mkdirs() }
            dir.listFiles()?.forEach { it.delete() } // one load at a time: the previous master is not needed any more
            val file = File(dir, "master-${System.nanoTime()}.m3u8")
            file.writeText(filtered)
            android.util.Log.i("mpv", "HLS master filtered for libmpv: ${text.lines().count { it.startsWith("#EXT-X-MEDIA") }} -> " +
                "${filtered.lines().count { it.startsWith("#EXT-X-MEDIA") }} renditions")
            file.absolutePath
        }
    }.getOrNull()

    /**
     * BUG-080: FFmpeg opens a local master with `protocol_whitelist=file,crypto,data` and then refuses its absolute http(s) playlist
     * URLs ("Protocol 'http' not on whitelist"). The filtered master from [filteredMaster] is opened with the network protocols allowed;
     * every other load keeps FFmpeg's default (empty value: mpv clears the option). Value for mpv's `demuxer-lavf-o` (a key/value list:
     * the comma-separated whitelist is quoted with `[...]`).
     */
    fun lavfOptions(playedUrl: String, sourceUrl: String): String =
        if (playedUrl != sourceUrl && isLocalPath(playedUrl)) "protocol_whitelist=[$LOCAL_MASTER_PROTOCOLS]" else ""

    private fun isLocalPath(url: String): Boolean = url.startsWith("/") || url.startsWith("file://")

    /** What a filtered master and its nested playlists, keys and segments need (the PC's proxy is http, CDNs are https). */
    const val LOCAL_MASTER_PROTOCOLS = "file,http,https,tcp,tls,crypto,data"

    private const val FETCH_TIMEOUT_S = 8L
}
