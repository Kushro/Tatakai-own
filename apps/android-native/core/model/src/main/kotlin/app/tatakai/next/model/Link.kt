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
