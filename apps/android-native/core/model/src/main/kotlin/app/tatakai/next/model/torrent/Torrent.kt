// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model.torrent

import app.tatakai.next.model.Media
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.StreamSource
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow

// Torrent model mirrored 1:1 from apps/desktop-native/crates/tk-ui/src/backend.rs (Torrent*Vm, docs/redesign/10 §5.2) and
// docs/redesign/11 §2.2-2.4, with the user's android-wave decisions (docs/redesign/12 §0). Signatures are FINAL.
// u32 -> Int, u64 -> Long, usize -> Int, f64 -> Double. JSON: camelCase, lowercase enum names (desktop serde).

/** Info-hash v1, lowercase hex (40). Stable across restarts. */
typealias TorrentId = String

const val TORRENT_METADATA_TIMEOUT_SECS: Long = 90

@Serializable
enum class TorrentState(val id: String) {
    @SerialName("initializing") INITIALIZING("initializing"),
    @SerialName("downloading") DOWNLOADING("downloading"),
    @SerialName("seeding") SEEDING("seeding"),
    @SerialName("paused") PAUSED("paused"),
    @SerialName("error") ERROR("error"),
}

/** Protocol pills of the Overview (`torrent_proto_<field>`). true = active. */
@Serializable
data class ProtocolStatus(
    val dht: Boolean = false,
    val lsd: Boolean = false,
    val pex: Boolean = false,
    val nat: Boolean = false,
    val forwarding: Boolean = false,
    val persisting: Boolean = false,
    val streaming: Boolean = false,
)

/** desktop `TorrentStatsVm`: [totalBytes] = selected files, [peers] = live connections. */
@Serializable
data class TorrentStats(
    val id: TorrentId,
    val name: String,
    val infoHash: String,
    val state: TorrentState,
    val error: String? = null,
    /** 0.0..=1.0 of the selected files. */
    val progress: Double = 0.0,
    val downloadedBytes: Long = 0,
    val uploadedBytes: Long = 0,
    val totalBytes: Long = 0,
    val sizeBytes: Long = 0,
    val pieces: Int = 0,
    val pieceBytes: Long = 0,
    val piecesHave: Int = 0,
    val downBps: Long = 0,
    val upBps: Long = 0,
    val seeders: Int = 0,
    val leechers: Int = 0,
    val peers: Int = 0,
    val etaSecs: Long? = null,
    val streaming: Boolean = false,
    val elapsedSecs: Long = 0,
    val protocol: ProtocolStatus = ProtocolStatus(),
)

@Serializable
data class TorrentFile(
    /** The torrent's file index (not a list position). */
    val index: Int,
    /** Path inside the torrent, `/`-separated. */
    val path: String,
    val name: String,
    val sizeBytes: Long,
    val downloadedBytes: Long = 0,
    val progress: Double = 0.0,
    val selected: Boolean = false,
    /** Open loopback streams right now. */
    val streams: Int = 0,
    /** Episode parsed from the file name (`parseRelease(name).episodes?.first`), for batches. */
    val episode: Int? = null,
)

@Serializable
data class TorrentPeer(
    /** `ip:port` */
    val addr: String,
    val client: String? = null,
    val progress: Double = 0.0,
    val downBps: Long = 0,
    val upBps: Long = 0,
    val downloadedBytes: Long = 0,
    val uploadedBytes: Long = 0,
    /** ISO 3166-1 alpha-2; null when unknown. */
    val country: String? = null,
    /** qBittorrent-style letters `D U I P E X H S`; legend keys `torrent_peers_flag_<letter lowercase>`. */
    val flags: String = "",
)

@Serializable
enum class TrackerStatus(val id: String) {
    @SerialName("working") WORKING("working"),
    @SerialName("updating") UPDATING("updating"),
    @SerialName("failed") FAILED("failed"),
    @SerialName("unknown") UNKNOWN("unknown"),
}

@Serializable
data class TorrentTracker(
    val url: String,
    val status: TrackerStatus = TrackerStatus.UNKNOWN,
    val message: String? = null,
    val downloaded: Int? = null,
    val seeders: Int? = null,
    val leechers: Int? = null,
)

/** Persisted by the engine (labels.json) per info-hash. [mediaId] = `HistoryEntry.animeIdOf(...)`. */
@Serializable
data class TorrentLabel(val mediaId: String, val title: String, val episode: Int? = null, val poster: String? = null)

/** One saved torrent (Library) or background download (Downloads). Newest first in [TorrentRepository.library]. */
@Serializable
data class TorrentLibraryItem(
    val id: TorrentId,
    val name: String,
    val mediaId: String? = null,
    val series: String? = null,
    val episode: Int? = null,
    val poster: String? = null,
    val files: Int = 0,
    val sizeBytes: Long = 0,
    val progress: Double = 0.0,
    val state: TorrentState = TorrentState.INITIALIZING,
    val addedUnix: Long = 0,
    /** Kept after watching (otherwise removed with its files when the session ends, after-watch policy). */
    val persist: Boolean = false,
    /** A file path (app-specific storage) or a `content://` URI after the SAF move-on-complete (11 §6). */
    val path: String = "",
    /** ANDROID-ONLY addition (12 §0): keep seeding after completion. Default false: Android never seeds after completing unless enabled per torrent. */
    val seed: Boolean = false,
)

@Serializable
enum class AfterWatch(val id: String) {
    @SerialName("keep") KEEP("keep"),
    @SerialName("delete") DELETE("delete"),
}

/** Where downloads end up (11 §6): app-specific external files dir (default) or a SAF tree the file is MOVED to when complete. */
@Serializable
sealed interface StorageLocation {
    @Serializable
    @SerialName("app")
    data object AppSpecific : StorageLocation

    @Serializable
    @SerialName("tree")
    data class UserFolder(val treeUri: String) : StorageLocation
}

/**
 * desktop `TorrentSettingsVm` + Android-only fields of 11 §2.4. Persisted by worker a inside [app.tatakai.next.model.AppSettings.torrent]
 * (DataStore); the engine (worker f) observes `SettingsRepository.settings` and applies limits live. Android defaults: maxPeers 30, upnp false.
 */
@Serializable
data class TorrentSettings(
    /** Unused on Android (always ""): [storageLocation] decides. Kept for 1:1 parity with desktop. */
    val downloadDir: String = "",
    /** KiB/s, 0 = unlimited, max [MAX_LIMIT_KIB]. */
    val downloadLimitKib: Int = 0,
    val uploadLimitKib: Int = 0,
    /** 0 = random, else 1024..=65535. */
    val listenPort: Int = 0,
    val upnp: Boolean = false,
    /** 1..=[MAX_PEERS]. */
    val maxPeers: Int = 30,
    val dht: Boolean = true,
    val pex: Boolean = true,
    val lsd: Boolean = true,
    val afterWatch: AfterWatch = AfterWatch.DELETE,
    /** Only honoured when [backgroundAck]. */
    val backgroundDownloads: Boolean = false,
    /** The user accepted the "Entiendo" warning. */
    val backgroundAck: Boolean = false,
    // --- Android only (11 §2.4) ---
    /** No background downloading without an unmetered network; playback on metered data asks first. */
    val wifiOnly: Boolean = true,
    /** Pause background downloads with battery < 20 % (not charging) or battery saver on. */
    val pauseOnLowBattery: Boolean = true,
    /** On metered networks the engine uploads at most 1 KiB/s unless this is true. */
    val uploadOnMobileData: Boolean = false,
    val storageLocation: StorageLocation = StorageLocation.AppSpecific,
) {
    /** Clamp into range, port 0 or 1024..=65535 (else 0), background off without ack. Implemented here (contract, 1:1 with desktop `sanitized`). */
    fun sanitized(): TorrentSettings = copy(
        downloadDir = downloadDir.trim(),
        maxPeers = maxPeers.coerceIn(1, MAX_PEERS),
        downloadLimitKib = downloadLimitKib.coerceIn(0, MAX_LIMIT_KIB),
        uploadLimitKib = uploadLimitKib.coerceIn(0, MAX_LIMIT_KIB),
        listenPort = if (listenPort == 0 || listenPort in 1024..65535) listenPort else 0,
        backgroundDownloads = backgroundDownloads && backgroundAck,
    )

    companion object {
        const val MAX_PEERS: Int = 500
        const val MAX_LIMIT_KIB: Int = 1_000_000
    }
}

/**
 * A torrent playback session (desktop `TorrentPlaybackVm`): [stream] has the loopback URL
 * `http://127.0.0.1:<port>/<token>/<infoHash>/<fileIndex>/<name>` with `container = Container.PROGRESSIVE` and no headers.
 */
data class TorrentPlayback(
    val args: PlayerArgs,
    val stream: StreamSource,
    val torrentId: TorrentId,
    val fileIndex: Int,
    val fileName: String,
    /** More than one video file: the next episode can reuse the torrent (`play(torrentId, media, ep + 1)`). */
    val batch: Boolean,
)

/** Sections of the client (desktop `model::TorrentSection`). Label keys `torrent_section_<id>` live in :core:ui. */
@Serializable
enum class TorrentSection(val id: String, val perTorrent: Boolean) {
    @SerialName("overview") OVERVIEW("overview", true),
    @SerialName("files") FILES("files", true),
    @SerialName("peers") PEERS("peers", true),
    @SerialName("trackers") TRACKERS("trackers", true),
    @SerialName("library") LIBRARY("library", false),
    @SerialName("downloads") DOWNLOADS("downloads", false),
    @SerialName("settings") SETTINGS("settings", false),
}

sealed interface TorrentEvent {
    val id: TorrentId

    data class Added(override val id: TorrentId) : TorrentEvent
    data class MetadataReady(override val id: TorrentId) : TorrentEvent
    data class StateChanged(override val id: TorrentId, val state: TorrentState) : TorrentEvent
    data class FileCompleted(override val id: TorrentId, val index: Int) : TorrentEvent
    data class Completed(override val id: TorrentId) : TorrentEvent
    data class StreamOpened(override val id: TorrentId, val index: Int) : TorrentEvent
    data class StreamClosed(override val id: TorrentId, val index: Int) : TorrentEvent
    data class Removed(override val id: TorrentId) : TorrentEvent
    data class Error(override val id: TorrentId, val message: String) : TorrentEvent
}

/** Live status of the engine / service: badge, network and battery chips, metered confirmation. */
data class TorrentServiceStatus(
    /** The engine is started (the service runs only while something downloads or streams). */
    val running: Boolean = false,
    /** Not completed, not paused, not streaming: the Library tab badge. */
    val activeDownloads: Int = 0,
    val streaming: Boolean = false,
    /** The default network is metered right now (cellular, metered Wi-Fi). */
    val metered: Boolean = false,
    /** Chip "Esperando Wi-Fi". */
    val waitingForWifi: Boolean = false,
    /** Chip "Pausado: batería baja". */
    val pausedLowBattery: Boolean = false,
    /** Android 15 `Service.onTimeout` of the dataSync type paused everything ("Descargas pausadas por el sistema"). */
    val pausedBySystem: Boolean = false,
)

data class TorrentStorage(val usedBytes: Long, val freeBytes: Long, val location: StorageLocation)

/**
 * Torrent engine facade (11 §2.2), implemented by worker f in `:core:torrent` with libtorrent4j 2.1.0-39 behind it (the engine can be
 * swapped later without touching callers). Bound in `torrentModule`. Settings come from `SettingsRepository.settings.value.torrent`.
 * Errors: `Result.failure(AppException(AppError...))` - no metadata within [TORRENT_METADATA_TIMEOUT_SECS] -> `AppError.Offline`;
 * no matching file -> `AppError.Unknown("torrent:no_file:<torrentId>")` (the UI opens Files of that torrent).
 */
interface TorrentRepository {
    val events: SharedFlow<TorrentEvent>

    /** Newest first. */
    val library: StateFlow<List<TorrentLibraryItem>>

    val status: StateFlow<TorrentServiceStatus>

    /**
     * Start (or reuse) a torrent and return the loopback stream of the episode file. [source] = magnet URI, 40-hex info-hash, or the id
     * of a torrent already in the session (batch next episode / client "Play this file"). Starts the service on demand; free-space check;
     * `pickEpisodeFile(files, episode, titles of [media])` unless [fileIndex] is given; sequential download + piece deadlines.
     */
    suspend fun play(source: String, media: Media, episode: Int, fileIndex: Int? = null): Result<TorrentPlayback>

    /** After-watch policy of desktop 10 §4.3 (20 s grace for batch next episode). Call when leaving the player or changing episode. */
    suspend fun playbackEnded(torrentId: TorrentId, completed: Boolean)

    /** Background download (persist = true). Fails when background downloads are not enabled/acknowledged. [episode] null = whole torrent. */
    suspend fun addDownload(source: String, media: Media, episode: Int?): Result<TorrentId>

    /** 1 s tick while collected; stops without collectors. */
    fun stats(id: TorrentId): Flow<TorrentStats>

    suspend fun files(id: TorrentId): List<TorrentFile>

    /** 1 s tick while collected; fastest first. */
    fun peers(id: TorrentId): Flow<List<TorrentPeer>>

    suspend fun trackers(id: TorrentId): List<TorrentTracker>

    suspend fun remove(id: TorrentId, deleteFiles: Boolean)

    suspend fun pause(id: TorrentId)

    suspend fun resume(id: TorrentId)

    suspend fun setPersist(id: TorrentId, persist: Boolean)

    /** Android-only: keep seeding this torrent after completion ([TorrentLibraryItem.seed]). */
    suspend fun setSeeding(id: TorrentId, seed: Boolean)

    /** Files tab "Download only this one": add [fileIndex] to the selected files (normal priority). */
    suspend fun downloadFile(id: TorrentId, fileIndex: Int)

    /** Downloaded ranges of the file as fractions 0..1 (seek-bar marks; poll every 2-3 s). */
    suspend fun seekMarks(id: TorrentId, fileIndex: Int): List<ClosedFloatingPointRange<Double>>

    /** "Guardar en Descargas": copy a completed file to MediaStore Downloads; returns the new content URI. */
    suspend fun exportToDownloads(id: TorrentId, fileIndex: Int): Result<String>

    suspend fun storageUsage(): TorrentStorage
}
