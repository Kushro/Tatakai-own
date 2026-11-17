// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.util.TreeMap

// parity wave A (docs/redesign/15 §3.2, worker p): subtitle helpers of the player screen. Bound in `playerModule`.

/**
 * What the screen needs to draw subtitles itself and to translate them: fetch + parse external tracks (cached per URL for the life of
 * the app), copy a file picked with SAF to `cacheDir/subs/` (mpv `sub-add` needs a path), the translation cache and the font folder.
 */
class SubtitleTools(context: Context, private val client: OkHttpClient) {
    private val appContext = context.applicationContext
    val filesDir: File = File(appContext.cacheDir, "subs")
    val translationCache: TranslationCache = TranslationCache(File(appContext.cacheDir, "subs-translate"))
    val fonts: SubtitleFonts = SubtitleFonts(appContext)
    private val mutex = Mutex()
    private val parsed = LinkedHashMap<String, List<TimedCue>>()

    /** Downloads and parses the external track at [url] with the stream's [headers]. Throws [IOException] on HTTP / parse failure. */
    suspend fun fetch(url: String, headers: Map<String, String>, format: String? = null): List<TimedCue> {
        mutex.withLock { parsed[url] }?.let { return it }
        val cues = withContext(Dispatchers.IO) {
            val text = if (url.startsWith("file:") || url.startsWith("/")) {
                File(Uri.parse(url).path ?: url).readText()
            } else {
                val req = Request.Builder().url(url).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
                client.newCall(req).execute().use { res ->
                    if (!res.isSuccessful) throw IOException("HTTP ${res.code}")
                    val body = res.body.bytes()
                    if (body.size > MAX_SUBTITLE_BYTES) throw IOException("subtitle too large")
                    String(body, Charsets.UTF_8)
                }
            }
            SubtitleParser.parse(text, format ?: SubtitleParser.formatOf(url))
        }
        if (cues.isEmpty()) throw IOException("no cues")
        mutex.withLock {
            parsed[url] = cues
            while (parsed.size > MAX_PARSED_TRACKS) parsed.remove(parsed.keys.first())
        }
        return cues
    }

    /** Copies a SAF document to `cacheDir/subs/<name>` and parses it. Returns the file and its cues. */
    suspend fun import(uri: Uri): Pair<File, List<TimedCue>> = withContext(Dispatchers.IO) {
        val name = displayName(uri) ?: "subtitle.srt"
        val format = SubtitleParser.formatOf(name) ?: appContext.contentResolver.getType(uri)?.let { SubtitleParser.formatOf(it) }
        val safe = name.replace(Regex("[^A-Za-z0-9._ -]"), "_").take(80).let { if (SubtitleParser.formatOf(it) == null) "$it.${format ?: "srt"}" else it }
        filesDir.mkdirs()
        val target = File(filesDir, safe)
        val input = appContext.contentResolver.openInputStream(uri) ?: throw IOException("cannot open")
        input.use { src -> target.outputStream().use { src.copyTo(it) } }
        if (target.length() > MAX_SUBTITLE_BYTES) {
            target.delete()
            throw IOException("subtitle too large")
        }
        val cues = SubtitleParser.parse(target.readText(), format)
        if (cues.isEmpty()) {
            target.delete()
            throw IOException("no cues")
        }
        parsed[target.toURI().toString()] = cues
        target to cues
    }

    /** Translator of [config] on the player's HTTP client. Throws [TranslationException] for missing fields. */
    fun translator(config: TranslatorConfig): SubtitleTranslator = SubtitleTranslators.create(config, client)

    /**
     * 0.4.0 (sub-translation): random access to the playing file for [EmbeddedSubtitleExtractor]. [torrentRanges] = the downloaded
     * parts of a torrent file as fractions 0..1 (`TorrentRepository.seekMarks`), null when the file is complete / not a torrent.
     * Null for streaming manifests (HLS / DASH: see [HlsSubtitles]) and unknown schemes.
     */
    fun mediaSource(url: String, headers: Map<String, String>, torrentRanges: (() -> List<ClosedFloatingPointRange<Double>>)?): MediaByteSource? {
        val lower = url.lowercase().substringBefore('?')
        if (lower.endsWith(".m3u8") || lower.endsWith(".mpd")) return null
        val availability: ((Long) -> List<LongRange>?)? = torrentRanges?.let { marks -> { length: Long -> TorrentByteRanges.of(marks(), length) } }
        return when {
            url.startsWith("/") || url.startsWith("file:") -> {
                val path = if (url.startsWith("/")) url else Uri.parse(url).path ?: return null
                val raf = runCatching { java.io.RandomAccessFile(path, "r") }.getOrNull() ?: return null
                ChannelByteSource(raf.channel, raf, availability?.let { a -> { a(raf.length()) } })
            }
            url.startsWith("content:") -> {
                val pfd = runCatching { appContext.contentResolver.openFileDescriptor(Uri.parse(url), "r") }.getOrNull() ?: return null
                val channel = java.io.FileInputStream(pfd.fileDescriptor).channel
                ChannelByteSource(channel, pfd, availability?.let { a -> { a(pfd.statSize) } })
            }
            url.startsWith("http://") || url.startsWith("https://") -> {
                val host = Uri.parse(url).host.orEmpty()
                val loopback = host == "127.0.0.1" || host == "localhost" || host == "::1" || host == "[::1]"
                HttpByteSource(client, url, headers, remote = !loopback, availability = availability)
            }
            else -> null
        }
    }

    /** 0.4.0 (sub-translation): cues of the HLS subtitle rendition the player selected; null when the stream has none. */
    suspend fun hlsSubtitles(masterUrl: String, headers: Map<String, String>, want: EmbeddedTrackRef): List<TimedCue>? =
        HlsSubtitles.fetch(client, masterUrl, headers, want)

    fun displayName(uri: Uri): String? =
        runCatching {
            appContext.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                if (c.moveToFirst()) c.getString(0) else null
            }
        }.getOrNull() ?: uri.lastPathSegment?.substringAfterLast('/')

    companion object {
        const val MAX_SUBTITLE_BYTES = 8L * 1024 * 1024
        const val MAX_PARSED_TRACKS = 12
    }
}

/** 0.4.0 (sub-translation): torrent piece fractions -> byte ranges that are fully downloaded (rounded inwards). Pure. */
object TorrentByteRanges {
    fun of(fractions: List<ClosedFloatingPointRange<Double>>, length: Long): List<LongRange> {
        if (length <= 0) return emptyList()
        return fractions.mapNotNull { r ->
            val start = kotlin.math.ceil(r.start.coerceIn(0.0, 1.0) * length).toLong()
            val endExclusive = if (r.endInclusive >= 1.0) length else kotlin.math.floor(r.endInclusive.coerceIn(0.0, 1.0) * length).toLong()
            if (endExclusive > start) start until endExclusive else null
        }.sortedBy { it.first }
    }
}

/**
 * Embedded (in-stream) Media3 cues shifted by a positive subtitle delay: the screen records every change of the visible lines at the
 * position it happened and, with a delay `d`, shows what was visible at `position - d`. A negative delay needs future cues, which only
 * parsed external tracks have ([SubtitleParser.activeAt]). Records older than [MAX_GAP_MS] before the looked-up position are ignored, so
 * a seek never shows a stale line. Pure, no Android.
 */
class CueTimeline {
    private val changes = TreeMap<Long, List<String>>()

    fun record(positionMs: Long, lines: List<String>) {
        changes[positionMs.coerceAtLeast(0)] = lines
        while (changes.size > MAX_ENTRIES) changes.pollFirstEntry()
    }

    fun clear() = changes.clear()

    fun linesAt(positionMs: Long): List<String> {
        if (positionMs < 0) return emptyList()
        val e = changes.floorEntry(positionMs) ?: return emptyList()
        return if (positionMs - e.key > MAX_GAP_MS) emptyList() else e.value
    }

    companion object {
        const val MAX_GAP_MS = 10_000L
        const val MAX_ENTRIES = 2_000
    }
}
