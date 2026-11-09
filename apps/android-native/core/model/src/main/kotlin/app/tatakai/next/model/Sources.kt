// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import app.tatakai.next.model.torrent.DebridStatus
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Source model mirrored 1:1 from apps/desktop-native/crates/tk-core/src/sources.rs (docs/redesign/08 §3, 10 §3, 12 §3.1).
// JSON = serde camelCase of desktop, so contracts/fixtures/sources/sources-for.json decodes into these types.
// Signatures are the android wave contract (FINAL). Bodies marked TODO belong to worker a.

/** Reliability of an `(extId, label)` pair never seen before. */
const val INITIAL_RELIABILITY: Float = 0.8f

@Serializable
enum class SourceKind(val id: String) {
    @SerialName("web") WEB("web"),
    @SerialName("torrent") TORRENT("torrent"),
}

/** Vertical resolution class. [UNKNOWN] is first so it sorts below every real quality (Ord of desktop). */
@Serializable
enum class Quality {
    @SerialName("unknown") UNKNOWN,
    @SerialName("480p") P480,
    @SerialName("720p") P720,
    @SerialName("1080p") P1080,
    @SerialName("1440p") P1440,
    @SerialName("2160p") P2160,
    ;

    /** `Q` of sources.js: Unknown 0, 480p 1, 720p 2, 1080p 3, 1440p 4, 2160p 5. */
    fun rank(): Int = ordinal

    /** Display label (`1080p`); empty for [UNKNOWN]. */
    fun label(): String = when (this) {
        UNKNOWN -> ""
        P480 -> "480p"
        P720 -> "720p"
        P1080 -> "1080p"
        P1440 -> "1440p"
        P2160 -> "2160p"
    }

    companion object {
        /** Lenient parse of what extensions send (`"1080p"`, `"1080"`, `"FHD"`, `"4K"` ...); anything else is [UNKNOWN]. Port of `Quality::parse`. */
        fun parse(s: String?): Quality = parseQuality(s)
    }
}

@Serializable
data class WebInfo(
    val url: String,
    val headers: Map<String, String> = emptyMap(),
    /** Last measured time to first byte; null = not measured (no latency penalty). */
    val latencyMs: Int? = null,
    val subtitles: List<SubtitleTrack> = emptyList(),
    /**
     * parity wave B (16 §3.3, worker r): [url] is an embed page (extension `type == "embed"`), not a media URL. The candidate is listed
     * with `container = EMBED_CONTAINER_LABEL`, `toStream()` returns null and the player resolves it through `EmbedSniffer` first.
     */
    val embed: Boolean = false,
)

@Serializable
data class TorrentInfo(
    /** Hex v1 info-hash (40 chars, lowercase). */
    val infoHash: String,
    val sizeBytes: Long? = null,
    val seeders: Int = 0,
    val leechers: Int = 0,
    val magnet: String? = null,
)

/** One entry of the source selector. Exactly one of [web] / [torrent] is non-null, matching [kind]. */
@Serializable
data class SourceCandidate(
    val extId: String,
    val extName: String,
    val kind: SourceKind,
    /** Server ("Alpha", "Ok.ru") for web, release group ("SubsPlease") for torrent. */
    val label: String,
    val quality: Quality = Quality.UNKNOWN,
    /** Display string: "HLS", "MP4", "HEVC 10-bit" ... */
    val container: String? = null,
    /** ISO 639-1 lowercase codes. */
    val subs: List<String> = emptyList(),
    val dubs: List<String> = emptyList(),
    /** 0.0..=1.0 from [SourceStatsStore.reliability]; [INITIAL_RELIABILITY] when unknown. */
    val reliability: Float = INITIAL_RELIABILITY,
    /** "dub latino", "multi-sub", "mini". */
    val note: String? = null,
    val web: WebInfo? = null,
    val torrent: TorrentInfo? = null,
    /** Release tags (torrent names); default (empty) for web sources. */
    val tags: ReleaseTags = ReleaseTags(),
    /**
     * parity wave A (2026-11-09 amendment, worker q): the torrent as seen by a linked debrid service (`DebridRepository.cached`); with
     * `cached = true` it plays as a direct HTTPS stream instead of the engine. null = no debrid. Mirrors desktop `SourceVm.debrid: Option<DebridStatusVm>`.
     */
    val debrid: DebridStatus? = null,
) {
    /** parity wave B (16 §3.3): a web candidate whose URL is an embed page (needs the sniffer before it can play). */
    val isEmbed: Boolean get() = kind == SourceKind.WEB && web?.embed == true

    /** The playable stream of a web candidate; null for torrents and for embed pages (see `EmbedSniffer`). Port of `SourceCandidate::to_stream`. */
    fun toStream(): StreamSource? {
        if (kind != SourceKind.WEB) return null
        val w = web ?: return null
        if (w.embed) return null
        val c = container?.let(::parseContainer) ?: containerFromUrl(w.url) ?: return null
        return StreamSource(
            url = w.url,
            headers = w.headers,
            container = c,
            quality = quality.label().takeIf { it.isNotEmpty() },
            provider = label,
            extensionId = extId,
            subtitles = w.subtitles,
        )
    }

    companion object {
        /** Wrap one raw extension stream as a web candidate. Port of `SourceCandidate::from_stream`. */
        fun fromStream(s: StreamSource, extName: String): SourceCandidate = SourceCandidate(
            extId = s.extensionId,
            extName = extName,
            kind = SourceKind.WEB,
            label = s.provider,
            quality = s.quality?.let { Quality.parse(it) } ?: Quality.UNKNOWN,
            container = containerLabel(s.container),
            // external subtitle tracks are the only language data a plain StreamSource carries
            subs = normalizeLangCodes(s.subtitles.map { it.lang }),
            dubs = emptyList(),
            reliability = INITIAL_RELIABILITY,
            note = null,
            web = WebInfo(url = s.url, headers = s.headers, latencyMs = null, subtitles = s.subtitles),
            torrent = null,
            tags = ReleaseTags(),
        )
    }
}

@Serializable
enum class SourceMode {
    @SerialName("auto") AUTO,
    @SerialName("manual") MANUAL,
}

/** Defaults: Auto / any kind / 720p / subs `es` / audio `ja` (desktop `SourcePrefs::default`). */
@Serializable
data class SourcePrefs(
    val mode: SourceMode = SourceMode.AUTO,
    /** null = any kind. */
    val prefer: SourceKind? = null,
    val minQuality: Quality = Quality.P720,
    val subLang: String = "es",
    val dubLang: String = "ja",
    /** parity wave B (16 §3.3, checklist sources.embed-sniffer.ranking): list embed sources (sniffed in the hidden WebView). Off = hidden. */
    val allowEmbed: Boolean = true,
    /** parity wave B (16 §3.3, checklist sources.embed-sniffer.timeout-setting): seconds the sniffer waits for the first media request, 5..60. */
    val embedTimeoutSecs: Int = EMBED_TIMEOUT_DEFAULT_SECS,
) {
    /** Clamps the ranges (SettingsCodec applies it on read and write). */
    fun sanitized(): SourcePrefs = copy(embedTimeoutSecs = embedTimeoutSecs.coerceIn(EMBED_TIMEOUT_MIN_SECS, EMBED_TIMEOUT_MAX_SECS))

    companion object {
        const val EMBED_TIMEOUT_MIN_SECS: Int = 5
        const val EMBED_TIMEOUT_MAX_SECS: Int = 60
        const val EMBED_TIMEOUT_DEFAULT_SECS: Int = 20
    }
}

/** One ranked row of the selector (what desktop calls `SourceVm`): the candidate plus ranking facts. */
data class RankedSource(
    val candidate: SourceCandidate,
    val score: Float,
    /** False when the extension is disabled or unsupported: the row is shown disabled. */
    val extEnabled: Boolean = true,
    /** The first entry of the ranked list (the one Auto mode plays). */
    val recommended: Boolean = false,
    /** The user's manual pick for this episode ([SourceStatsStore.pickedSource]). */
    val picked: Boolean = false,
) {
    /** [sourceKey] of [candidate]: pass it to `reportSourceResult`, `setPickedSource` and `PlayerArgs.sourceKey`. */
    val key: String get() = sourceKey(candidate)
}

/** Stable id `"<extId>|<kind>|<label>"` (kind = [SourceKind.id]). Implemented here (contract). */
fun sourceKey(c: SourceCandidate): String = "${c.extId}|${c.kind.id}|${c.label}"

/** Inverse of [sourceKey]: split on the first two `|` (the label may contain more). null if malformed. Implemented here (contract). */
fun parseSourceKey(key: String): Triple<String, SourceKind, String>? {
    val parts = key.split('|', limit = 3)
    if (parts.size < 3 || parts[0].isEmpty()) return null
    val kind = when (parts[1]) {
        "web" -> SourceKind.WEB
        "torrent" -> SourceKind.TORRENT
        else -> return null
    }
    return Triple(parts[0], kind, parts[2])
}

/**
 * What `TorrentRepository.play` / `addDownload` take as `source` for a torrent candidate: its magnet, else
 * `magnet:?xt=urn:btih:<infoHash>`; null for web candidates. Implemented here (contract).
 */
fun SourceCandidate.torrentUri(): String? {
    val t = torrent ?: return null
    return t.magnet?.takeIf { it.isNotBlank() } ?: "magnet:?xt=urn:btih:${t.infoHash}"
}

/**
 * Persistence of per-source facts (desktop `LocalStore` reliability / picked source). Implemented by worker a in `:core:data`
 * (bound in `dataModule`), consumed by worker e's `ExtensionRepositoryImpl` and worker d's player. [mediaId] is
 * `HistoryEntry.animeIdOf(anilistId, malId)`.
 */
interface SourceStatsStore {
    /** [INITIAL_RELIABILITY] when there is no row. */
    suspend fun reliability(extId: String, label: String): Float

    /** Applies `updateReliability` and returns the new value. */
    suspend fun recordSourceResult(extId: String, label: String, ok: Boolean): Float

    /** The [sourceKey] picked manually for this episode, if any. */
    suspend fun pickedSource(mediaId: String, episode: Int): String?

    suspend fun setPickedSource(mediaId: String, episode: Int, key: String)

    suspend fun clearPickedSource(mediaId: String, episode: Int)
}
