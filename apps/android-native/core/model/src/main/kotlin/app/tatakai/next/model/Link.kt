package app.tatakai.next.model

/** Tatakai Link v1 (contracts/link/openapi.v1.json). */
data class DiscoveredPc(val name: String, val host: String, val port: Int, val instanceId: String? = null)

data class PairedPc(val deviceId: String, val serverName: String, val host: String, val port: Int)

data class LinkProfile(
    val id: String,
    val label: String,
    val upscale: Boolean,
    val frameGeneration: Boolean,
    val outputWidth: Int,
    val outputHeight: Int,
    /** "true" | "false" | "unknown" */
    val measuredSustain: String,
    /** BUG-132: fixed frame-generation multiplier (`fg_multiplier`; output fps = source fps x n); null = none or unknown. */
    val fgMultiplier: Int? = null,
    /** BUG-132: LSFG automatic profiles aim at this output frame rate (`fg_target_fps`). */
    val fgTargetFps: Int? = null,
)

data class LinkCapabilities(
    val serverName: String,
    val serverVersion: String,
    val instanceId: String,
    val profiles: List<LinkProfile>,
    val defaultProfile: String,
    val clientFrameGenerationAllowed: Boolean,
)

sealed interface LinkSource {
    /** A file from the PC library (32-hex item id). */
    data class Library(val itemId: String) : LinkSource

    /** An external stream resolved by an extension on the phone; the PC fetches it with SSRF guards. */
    data class Url(val url: String, val headers: Map<String, String> = emptyMap()) : LinkSource

    /**
     * BUG-009: a torrent the PC adds to its own engine and transcodes ([uri] = magnet or info-hash; [mediaId] = AniList id and
     * [episode] pick the file of a pack). Needs a PC with the Link `torrent` source (older PCs answer request_invalid).
     */
    data class Torrent(val uri: String, val mediaId: String, val episode: Int) : LinkSource
}

data class RemoteSession(
    val sessionId: String,
    val generation: Int,
    /** Absolute playback URL (master.m3u8) on the paired PC. */
    val playbackUrl: String,
    val durationSecs: Double?,
    val heartbeatIntervalSecs: Int,
    val idleTimeoutSecs: Int,
)

sealed interface LinkState {
    data object Unpaired : LinkState
    data object Pairing : LinkState
    data class Paired(val pc: PairedPc, val reachable: Boolean) : LinkState
    data class Failed(val message: String) : LinkState
}

/** android wave (12 §3.5): the paired PC as the "Dispositivo enlazado" screen shows it. */
data class LinkedDevice(
    val deviceId: String,
    val serverName: String,
    val host: String,
    val port: Int,
    /** CA certificate fingerprint, display form (`7A3F 91C2 ...`). */
    val fingerprint: String?,
    val pairedAtMs: Long?,
    /** Last successful request to the PC; null = never after pairing. */
    val lastSeenMs: Long?,
    /** Enhance profile chosen on the phone (`LinkProfile.id`); null = the PC's default. */
    val preferredProfile: String?,
    /** Name this phone registered with at pairing. */
    val localDeviceName: String?,
)
