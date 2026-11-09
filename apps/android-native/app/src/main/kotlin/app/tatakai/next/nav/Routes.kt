package app.tatakai.next.nav

import androidx.navigation3.runtime.NavKey
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentSection
import kotlinx.serialization.Serializable

/** Navigation3 back stack keys (04 §3.2): Home, Search, Details(kind, anilistId), Player(args), Library, Settings, Extensions, Link. */
@Serializable
sealed interface Route : NavKey {
    @Serializable data object Home : Route

    @Serializable data object Search : Route

    @Serializable data class Details(val kind: MediaKind, val anilistId: Int) : Route

    @Serializable data class Player(val args: PlayerArgs) : Route

    @Serializable data object Library : Route

    @Serializable data object Settings : Route

    @Serializable data object Extensions : Route

    @Serializable data object Link : Route

    // --- android wave contract (docs/redesign/12 §3.15). Owner: worker b. ---

    /** "Tú" (compact bottom bar). */
    @Serializable data object You : Route

    /** "Dispositivo enlazado" (Link client). Route.Link renders the same LinkedDeviceScreen. */
    @Serializable data object Linked : Route

    /** Torrent client: Library tab target on phones, rail destination on medium/expanded. */
    @Serializable data class Torrents(val section: TorrentSection = TorrentSection.LIBRARY, val torrentId: String? = null) : Route

    /** Settings deep link (scroll to a section; TORRENTS opens the Torrents subpage). */
    @Serializable data class SettingsSection(val section: app.tatakai.next.model.SettingsSection) : Route

    /**
     * Search pre-filled from Home (feed "Ver todo", genre chip, extension row item). parity wave A (15 §3.1): [studio] browses a
     * studio / producer (Details technical sheet chip).
     */
    @Serializable data class SearchFor(
        val query: String? = null,
        val feed: FeedKind? = null,
        val genre: String? = null,
        val studio: String? = null,
    ) : Route

    /** Library opened on a given tab (You › Historial). */
    @Serializable data class LibraryTab(val tab: app.tatakai.next.feature.library.LibraryTab) : Route

    /**
     * PC extension server connect / manage screen (docs/redesign/13 §3.10), reached from Linked device and from the
     * `tatakai://extensionserver/connect` deep link (the link itself waits in PendingServerLink, never in the back stack).
     */
    @Serializable data object ExtensionServers : Route

    // --- parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.6). Owner: contract / integrator. ---

    /** Character page (classic `/character/:name`), from the Characters tab of Details. Detail pane on expanded windows. Screen: worker o. */
    @Serializable data class Character(val characterId: Int) : Route

    /** Image search with trace.moe (classic `/image-search`); [imageUri] when opened by sharing a picture to the app. Screen: worker o. */
    @Serializable data class ImageSearch(val imageUri: String? = null) : Route

    // --- parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.12). Owner: contract / integrator. ---

    /** Downloads page (web + torrents + chapters + local library), rail destination on wide windows; You › Descargas on phones. Screen: r. */
    @Serializable data class Downloads(val tab: DownloadsTab = DownloadsTab.ACTIVE) : Route

    /** Airing calendar (plan §7 D6: inside Inicio), from the Home row header. Screen: r (:feature:home). */
    @Serializable data object Calendar : Route

    /** "Para ti" recommendations + atlas, from the Home row header. Screen: r (:feature:home). */
    @Serializable data object ForYou : Route

    /** Custom provider page of one extension source (`customSources`). Screen: r (:feature:extensions). */
    @Serializable data class Provider(val extensionId: String, val sourceId: String) : Route

    /** A provider title (`custom/info`) with its episodes / chapters. Screen: r (:feature:extensions). */
    @Serializable data class ProviderItem(val extensionId: String, val sourceId: String, val itemId: String) : Route

    /** Manga home (+ search when [query] is set: Search type chip). Rail destination on wide windows. Screen: s. Details: `Details(MANGA, id)`. */
    @Serializable data class Manga(val query: String? = null) : Route

    /** Manga reader; shown without the nav bar like Player. Screen: s. */
    @Serializable data class MangaReader(val args: MangaReaderArgs) : Route

    /** Novels home (+ search when [query] is set). Rail destination on wide windows. Screen: s. */
    @Serializable data class Novels(val query: String? = null) : Route

    /** Novel details (AniList MANGA with format NOVEL). Detail pane on expanded windows. Screen: s. */
    @Serializable data class NovelDetails(val anilistId: Int) : Route

    /** Novel reader; shown without the nav bar like Player. Screen: s. */
    @Serializable data class NovelReader(val args: NovelReaderArgs) : Route

    /** AniList profile: null = the linked viewer (rail destination on wide windows), else a public user. Screen: t. */
    @Serializable data class Profile(val userName: String? = null) : Route

    /** One AniList activity with its replies. Screen: t. */
    @Serializable data class ActivityThread(val activityId: Int) : Route

    /** AniList notifications. Screen: t. */
    @Serializable data object Notifications : Route
}

/** Compact (bottom bar, 4 items): Inicio · Buscar · Biblioteca · Tú. Torrents is a tab inside Library (11 §3.1, 12 §0). */
val TopLevelRoutesCompact: List<Route> = listOf(Route.Home, Route.Search, Route.Library, Route.You)

/**
 * Medium / expanded (rail): Inicio · Buscar · Biblioteca · Manga · Novelas · Descargas · Torrents · Extensiones · Enlace · Perfil · Ajustes
 * (desktop rail order; parity wave B adds Manga, Novelas, Descargas and Perfil).
 */
val TopLevelRoutesWide: List<Route> = listOf(
    Route.Home, Route.Search, Route.Library, Route.Manga(), Route.Novels(), Route.Downloads(), Route.Torrents(),
    Route.Extensions, Route.Linked, Route.Profile(), Route.Settings,
)

/** Top-level destinations for the current window class. */
fun topLevelRoutes(wide: Boolean): List<Route> = if (wide) TopLevelRoutesWide else TopLevelRoutesCompact

/** Pre-wave destinations (kept for source compatibility; the shell uses [topLevelRoutes]). */
val TopLevelRoutes: List<Route> = listOf(Route.Home, Route.Search, Route.Library, Route.Settings)

/** The compact top-level destination that owns this route (same as `topLevel(wide = false)`). */
fun Route.topLevel(): Route = topLevel(wide = false)

/**
 * The top-level destination that owns this route, used to highlight the nav item (12 §3.15).
 * Compact: Torrents -> Library; Extensions / Link / Linked / ExtensionServers / Settings / SettingsSection -> You.
 * Wide: every rail destination is its own (Torrents(*) -> Torrents(), Link / ExtensionServers -> Linked, SettingsSection / You -> Settings).
 * Routes without an owner (Details, Player) return themselves; the shell then highlights the entry below them in the back stack.
 */
fun Route.topLevel(wide: Boolean): Route = when (this) {
    is Route.SearchFor, is Route.ImageSearch -> Route.Search
    is Route.LibraryTab -> Route.Library
    is Route.Torrents -> if (wide) Route.Torrents() else Route.Library
    Route.Extensions, is Route.Provider, is Route.ProviderItem -> if (wide) Route.Extensions else Route.You
    Route.Link, Route.Linked, Route.ExtensionServers -> if (wide) Route.Linked else Route.You
    Route.Settings, is Route.SettingsSection -> if (wide) Route.Settings else Route.You
    Route.You -> if (wide) Route.Settings else Route.You
    // parity wave B (16 §3.12): compact = where the user came from (Home chips / You rows); wide = own rail destination.
    Route.Calendar, Route.ForYou -> Route.Home
    is Route.Downloads -> if (wide) Route.Downloads() else Route.You
    is Route.Manga, is Route.MangaReader -> if (wide) Route.Manga() else Route.Home
    is Route.Novels, is Route.NovelDetails, is Route.NovelReader -> if (wide) Route.Novels() else Route.Home
    is Route.Profile, is Route.ActivityThread, Route.Notifications -> if (wide) Route.Profile() else Route.You
    else -> this
}
