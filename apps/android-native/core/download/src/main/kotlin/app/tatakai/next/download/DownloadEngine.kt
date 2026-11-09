// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.content.Context
import android.net.Uri
import androidx.documentfile.provider.DocumentFile
import androidx.media3.common.MimeTypes
import androidx.media3.common.util.UnstableApi
import androidx.media3.common.util.UriUtil
import androidx.media3.database.StandaloneDatabaseProvider
import androidx.media3.datasource.DataSource
import androidx.media3.datasource.DataSourceInputStream
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.ResolvingDataSource
import androidx.media3.datasource.cache.CacheDataSource
import androidx.media3.datasource.cache.NoOpCacheEvictor
import androidx.media3.datasource.cache.SimpleCache
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.hls.playlist.HlsMediaPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsMultivariantPlaylist
import androidx.media3.exoplayer.hls.playlist.HlsPlaylistParser
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.scheduler.Requirements
import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadProfile
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import javax.crypto.Cipher
import javax.crypto.spec.IvParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Media3 side of the web downloads (16 §3.2, worker r): one `DownloadManager` over a `DefaultDownloadIndex` (`StandaloneDatabaseProvider`)
 * and a `SimpleCache` in `<download folder>/.cache`, fed by OkHttp through a `ResolvingDataSource` that adds the extension headers of
 * each download (registered by host: [registerHeaders]). HLS = `HlsDownloader` on the chosen media playlist (the variant is picked
 * here by the profile, [resolveHls]); MP4 = `ProgressiveDownloader` (Range, resumable). When Media3 reports COMPLETED, [export] writes
 * ONE playable file: HLS segments concatenated in order (AES-128 segments decrypted; fMP4 init segment first), MP4 copied, read back
 * through the cache (network fallback for anything missing).
 */
@UnstableApi
internal class DownloadEngine(private val context: Context, private val http: OkHttpClient) {
    /** host -> headers of the extension that produced the stream (Referer / Origin / cookies / UA). */
    private val headersByHost = ConcurrentHashMap<String, Map<String, String>>()

    val rootDir: File by lazy { context.getExternalFilesDir(DownloadRepositoryImpl.DIR_NAME) ?: File(context.filesDir, DownloadRepositoryImpl.DIR_NAME) }

    private val databaseProvider by lazy { StandaloneDatabaseProvider(context) }

    val cache: SimpleCache by lazy { SimpleCache(File(rootDir, CACHE_DIR), NoOpCacheEvictor(), databaseProvider) }

    private val upstream: DataSource.Factory by lazy {
        ResolvingDataSource.Factory(OkHttpDataSource.Factory(http)) { spec ->
            val host = spec.uri.host
            val extra = host?.let { headersByHost[it] }
            if (extra.isNullOrEmpty()) spec else spec.withAdditionalHeaders(extra)
        }
    }

    private val cacheFactory: CacheDataSource.Factory by lazy {
        CacheDataSource.Factory().setCache(cache).setUpstreamDataSourceFactory(upstream)
    }

    /** Created on first use; its callbacks run on the main looper (no looper on the calling thread = main). */
    val manager: DownloadManager by lazy {
        DownloadManager(context, databaseProvider, cache, upstream, Executors.newFixedThreadPool(DOWNLOAD_THREADS)).apply {
            minRetryCount = MIN_RETRIES
        }
    }

    fun registerHeaders(url: String, headers: Map<String, String>) {
        val host = runCatching { Uri.parse(url).host }.getOrNull() ?: return
        if (headers.isNotEmpty()) headersByHost[host] = headers
    }

    fun requirementsFor(wifiOnly: Boolean): Requirements =
        Requirements(if (wifiOnly) Requirements.NETWORK_UNMETERED else Requirements.NETWORK)

    /** Media3 mime of a container (DASH is rejected before reaching here). */
    fun mimeFor(container: Container): String = if (container == Container.HLS) MimeTypes.APPLICATION_M3U8 else MimeTypes.VIDEO_MP4

    /**
     * The media playlist to download for [url]: a multivariant master is fetched (with [headers]) and its variant picked by [profile];
     * a media playlist is returned as is. Blocking (call on IO).
     */
    fun resolveHls(url: String, headers: Map<String, String>, profile: DownloadProfile): String {
        val uri = Uri.parse(url)
        val playlist = fetch(url, headers) { HlsPlaylistParser().parse(uri, it) }
        if (playlist !is HlsMultivariantPlaylist) return url
        val variants = playlist.variants.mapIndexed { i, v ->
            DownloadLogic.Variant(i, v.format.height.takeIf { it > 0 }, maxOf(v.format.peakBitrate, v.format.averageBitrate, v.format.bitrate))
        }
        val pick = DownloadLogic.pickVariant(variants, profile) ?: throw IOException("hls: no variants")
        return playlist.variants[pick.index].url.toString()
    }

    private fun <T> fetch(url: String, headers: Map<String, String>, parse: (InputStream) -> T): T {
        val req = Request.Builder().url(url).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
        http.newCall(req).execute().use { resp ->
            if (!resp.isSuccessful) throw IOException("http ${resp.code}")
            return resp.body.byteStream().use(parse)
        }
    }

    /** Writes the playable file of a completed download to [out]; returns the bytes written. Blocking (IO). */
    fun export(requestUrl: String, container: Container, out: File): Long {
        out.parentFile?.mkdirs()
        val tmp = File(out.path + ".part")
        val written = tmp.outputStream().buffered().use { sink ->
            if (container == Container.HLS) exportHls(requestUrl, sink) else copy(DataSpec(Uri.parse(requestUrl)), sink)
        }
        if (out.exists()) out.delete()
        if (!tmp.renameTo(out)) throw IOException("rename failed")
        return written
    }

    private fun open(spec: DataSpec): InputStream = DataSourceInputStream(cacheFactory.createDataSource(), spec).also { it.open() }

    private fun copy(spec: DataSpec, sink: OutputStream): Long = open(spec).use { it.copyTo(sink, BUFFER) }

    private fun readAll(spec: DataSpec): ByteArray = open(spec).use { it.readBytes() }

    private fun exportHls(requestUrl: String, sink: OutputStream): Long {
        val uri = Uri.parse(requestUrl)
        val playlist = open(DataSpec(uri)).use { HlsPlaylistParser().parse(uri, it) } as? HlsMediaPlaylist
            ?: throw IOException("hls: not a media playlist")
        var total = 0L
        var lastInit: String? = null
        val keys = HashMap<String, ByteArray>()
        playlist.segments.forEachIndexed { index, segment ->
            segment.initializationSegment?.let { init ->
                val key = init.url + "@" + init.byteRangeOffset
                if (key != lastInit) {
                    total += writeSegment(playlist, init, playlist.mediaSequence + index, keys, sink)
                    lastInit = key
                }
            }
            total += writeSegment(playlist, segment, playlist.mediaSequence + index, keys, sink)
        }
        return total
    }

    private fun writeSegment(
        playlist: HlsMediaPlaylist,
        segment: HlsMediaPlaylist.SegmentBase,
        sequence: Long,
        keys: MutableMap<String, ByteArray>,
        sink: OutputStream,
    ): Long {
        val segUri = UriUtil.resolveToUri(playlist.baseUri, segment.url)
        val length = if (segment.byteRangeLength == -1L) androidx.media3.common.C.LENGTH_UNSET.toLong() else segment.byteRangeLength
        val spec = DataSpec(segUri, segment.byteRangeOffset, length)
        val keyUrl = segment.fullSegmentEncryptionKeyUri
        if (keyUrl == null) return copy(spec, sink)
        val keyUri = UriUtil.resolveToUri(playlist.baseUri, keyUrl)
        val key = keys.getOrPut(keyUri.toString()) { readAll(DataSpec(keyUri)) }
        val bytes = HlsCrypto.decrypt(readAll(spec), key, HlsCrypto.iv(segment.encryptionIV, sequence))
        sink.write(bytes)
        return bytes.size.toLong()
    }

    /** Copies [file] into the user's SAF folder (`<tree>/<dirName>/<name>`); returns the document uri. */
    fun copyToTree(treeUri: String, dirName: String, name: String, mime: String, file: File): String? {
        val tree = DocumentFile.fromTreeUri(context, Uri.parse(treeUri)) ?: return null
        val dir = tree.findFile(dirName)?.takeIf { it.isDirectory } ?: tree.createDirectory(dirName) ?: return null
        dir.findFile(name)?.delete()
        val doc = dir.createFile(mime, name) ?: return null
        context.contentResolver.openOutputStream(doc.uri)?.use { out -> file.inputStream().use { it.copyTo(out, BUFFER) } } ?: return null
        return doc.uri.toString()
    }

    /** (exists, length) of a `file://`, plain path or `content://` uri. */
    fun stat(uri: String): Pair<Boolean, Long> = when {
        uri.startsWith("content://") -> {
            val doc = DocumentFile.fromSingleUri(context, Uri.parse(uri))
            if (doc != null && doc.exists()) true to doc.length() else false to 0L
        }
        else -> {
            val f = File(Uri.parse(uri).path ?: uri)
            f.exists() to (if (f.exists()) f.length() else 0L)
        }
    }

    fun delete(uri: String) {
        runCatching {
            if (uri.startsWith("content://")) {
                DocumentFile.fromSingleUri(context, Uri.parse(uri))?.delete()
            } else {
                File(Uri.parse(uri).path ?: uri).delete()
            }
        }
    }

    fun cacheBytes(): Long = runCatching { cache.cacheSpace }.getOrDefault(0L)

    companion object {
        const val CACHE_DIR = ".cache"
        private const val DOWNLOAD_THREADS = 4
        private const val MIN_RETRIES = 5
        private const val BUFFER = 64 * 1024
    }
}

/** AES-128 (CBC, PKCS#7) of HLS `#EXT-X-KEY:METHOD=AES-128` segments (RFC 8216 §5.2). Pure, JVM-tested. */
internal object HlsCrypto {
    /** Explicit `IV=0x…` or, when absent, the media sequence number as a 16-byte big-endian integer. */
    fun iv(hex: String?, sequence: Long): ByteArray {
        val out = ByteArray(16)
        if (hex != null) {
            val clean = hex.removePrefix("0x").removePrefix("0X").padStart(32, '0').takeLast(32)
            for (i in 0 until 16) out[i] = clean.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        } else {
            for (i in 0 until 8) out[15 - i] = (sequence ushr (8 * i)).toByte()
        }
        return out
    }

    fun decrypt(data: ByteArray, key: ByteArray, iv: ByteArray): ByteArray {
        val cipher = Cipher.getInstance("AES/CBC/PKCS5Padding")
        cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), IvParameterSpec(iv))
        return cipher.doFinal(data)
    }
}
