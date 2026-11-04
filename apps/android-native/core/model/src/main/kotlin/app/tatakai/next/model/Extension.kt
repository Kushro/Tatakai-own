package app.tatakai.next.model

enum class ExtensionCompat { FULL, PARTIAL, UNSUPPORTED }

data class InstalledExtension(
    val id: String,
    val name: String,
    val version: String,
    val type: String,
    val permissions: List<String>,
    val compat: ExtensionCompat,
    val enabled: Boolean,
    // --- android wave (12 §3.4), all optional ---
    /** Why the compat level is not FULL (CompatReport.reason): the explanatory banner of the compatibility pill. */
    val compatReason: String? = null,
    val description: String = "",
    /** manifest.browserDomains ∩ network permissions: non-empty = the extension gets the hidden-WebView browser runtime. */
    val browserDomains: List<String> = emptyList(),
)

// --- android wave (12 §3.4) ---

enum class ExtLogLevel { OK, WARN, ERR }

/** One line of an extension's log (desktop `ExtLogVm`): op = install | search | stream | torrent | fetch | compat | browser | session ... */
data class ExtensionLogEntry(val level: ExtLogLevel, val op: String, val message: String, val atMs: Long?)

/** Extension details for the row (desktop `ExtensionDetailVm`). */
data class ExtensionDetail(
    val id: String,
    /** Hosts from the `network:domain:*` permissions. */
    val domains: List<String>,
    /** SHA-256 of the installed package, display form (`9f2c…b71e`). */
    val hash: String,
    val installedAtMs: Long?,
    /** Manifest `type` (`torrent`, `onlinestream`, `custom`). */
    val kind: String,
    val browserDomains: List<String> = emptyList(),
    /** Routes the host asks this extension for sources (`stream`, `torrent`). */
    val sourceRoutes: List<String> = emptyList(),
)

/** One lazy Home row from an extension's `custom/home` (prototype `customHome` row). Tapping an item searches its title. */
data class ExtensionHomeRow(val extensionId: String, val sourceId: String, val title: String, val items: List<ExtensionHomeItem>)

data class ExtensionHomeItem(val id: String, val title: String, val poster: String?)
