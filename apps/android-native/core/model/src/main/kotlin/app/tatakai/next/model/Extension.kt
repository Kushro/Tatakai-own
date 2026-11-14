package app.tatakai.next.model

enum class ExtensionCompat { FULL, PARTIAL, UNSUPPORTED }

/** BUG-094: what a `.kai` package says it is, read before installing it ([ExtensionRepository.peekPackage]). */
data class ExtensionPackageInfo(val id: String, val name: String, val version: String)

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

// --- parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t): updates, preferences, repositories, custom providers ---

/** A newer version of an installed extension found in one of the user's repositories (`ExtensionRepository.checkUpdates`). */
data class ExtensionUpdate(
    val id: String,
    val name: String,
    val installedVersion: String,
    val availableVersion: String,
    /** `.kai` URL to download. */
    val url: String,
    val notes: String? = null,
    val repositoryUrl: String,
)

enum class ExtensionPrefType { TEXT, SECRET, TOGGLE, SELECT }

/**
 * One entry of a manifest `preferences[]` (classic RemoteExtensionsPanel form). Values reach the bundle as `settings` of every call;
 * [ExtensionPrefType.SECRET] values are stored in the extension's state file (app-private), shown as "guardada" / "Quitar".
 */
data class ExtensionPreference(
    val key: String,
    val title: String,
    val type: ExtensionPrefType,
    val description: String? = null,
    /** SELECT options (`value` shown as is). */
    val options: List<String> = emptyList(),
    val default: String? = null,
)

/** One `.kai` listed by an extension repository index (`index.json`: `{ name, extensions: [{ id, name, version, description, url, notes? }] }`). */
data class ExtensionRepoEntry(
    val id: String,
    val name: String,
    val version: String,
    val description: String = "",
    val url: String,
    val installed: Boolean = false,
    val updateAvailable: Boolean = false,
)

/** An extension repository added by URL (checklist `ext.repositories`). */
data class ExtensionRepo(val url: String, val name: String, val entries: List<ExtensionRepoEntry>, val fetchedAtMs: Long?, val error: String? = null)

/** A custom source (manifest `customSources[]`) of an installed, enabled extension. [kind] = `watch` | `read` | `novel`. */
data class CustomSourceRef(
    val extensionId: String,
    val extensionName: String,
    val sourceId: String,
    val kind: String,
    val name: String,
    val icon: String? = null,
    val description: String? = null,
)

/**
 * BUG-099: what a route-based manga extension (`manga/chapters`, e.g. Toko 4.x, which has no `read` custom source) needs to find a
 * title: its AniList / MAL ids and its titles, best first.
 */
/** [CustomSourceRef.sourceId] of a route-based manga extension ([ExtensionRepository.mangaRouteSources]). */
const val MANGA_ROUTE_SOURCE: String = "manga-route"

/** [CustomSourceRef.sourceId] of a route-based novel extension ([ExtensionRepository.novelRouteSources], BUG-100). */
const val NOVEL_ROUTE_SOURCE: String = "novel-route"

data class MangaRouteQuery(val anilistId: Int?, val malId: Int?, val titles: List<String>)

/**
 * One chapter of a `manga/chapters` answer ([ExtensionRepository.mangaRouteChapters]). [provider] is the extension's inner provider
 * (`mangadex`, `comick`…: the `chapterKey` prefix); [key] is what `manga/pages` takes back (`chapterKey`).
 */
data class MangaRouteChapter(
    val provider: String,
    val key: String,
    val number: Double? = null,
    val volume: Int? = null,
    val title: String? = null,
    /** ISO 639-1, lowercase; null = unknown. */
    val lang: String? = null,
    val group: String? = null,
    val releasedAtMs: Long? = null,
)

/** One episode / chapter of a provider title (`custom/info` entry). [kind] = `watch` | `read` | `novel`. */
data class ProviderEntry(
    val id: String,
    val kind: String,
    val number: Double? = null,
    val title: String? = null,
    val releasedAtMs: Long? = null,
    val lang: String? = null,
)

/** A provider title page (`custom/info`, classic CustomInfoPage). */
data class ProviderItem(
    val id: String,
    val title: String,
    val poster: String? = null,
    val banner: String? = null,
    val description: String? = null,
    val status: String? = null,
    val genres: List<String> = emptyList(),
    val entries: List<ProviderEntry> = emptyList(),
    /** AniList match proposed by the host (title search), so Watch records history under a real id when possible. */
    val match: MediaRef? = null,
)

/** Bookmark of a provider title (classic CustomInfoPage "Bookmark" / "Mark completed"); stored by :core:extensions-quickjs. */
data class ProviderBookmark(
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val title: String,
    val poster: String? = null,
    val completed: Boolean = false,
    val addedAtMs: Long,
)
