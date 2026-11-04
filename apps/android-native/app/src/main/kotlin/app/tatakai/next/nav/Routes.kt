package app.tatakai.next.nav

import androidx.navigation3.runtime.NavKey
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.MediaKind
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

    /** Search pre-filled from Home (feed "Ver todo", genre chip, extension row item). */
    @Serializable data class SearchFor(val query: String? = null, val feed: FeedKind? = null, val genre: String? = null) : Route

    /** Library opened on a given tab (You › Historial). */
    @Serializable data class LibraryTab(val tab: app.tatakai.next.feature.library.LibraryTab) : Route
}

/** Compact (bottom bar, 4 items): Inicio · Buscar · Biblioteca · Tú. Torrents is a tab inside Library (11 §3.1, 12 §0). */
val TopLevelRoutesCompact: List<Route> = listOf(Route.Home, Route.Search, Route.Library, Route.You)

/** Medium / expanded (rail): Inicio · Buscar · Biblioteca · Torrents · Extensiones · Enlace · Ajustes (desktop rail order). */
val TopLevelRoutesWide: List<Route> =
    listOf(Route.Home, Route.Search, Route.Library, Route.Torrents(), Route.Extensions, Route.Linked, Route.Settings)

/** Top-level destinations for the current window class. */
fun topLevelRoutes(wide: Boolean): List<Route> = if (wide) TopLevelRoutesWide else TopLevelRoutesCompact

/** Pre-wave destinations (kept for source compatibility; the shell uses [topLevelRoutes]). */
val TopLevelRoutes: List<Route> = listOf(Route.Home, Route.Search, Route.Library, Route.Settings)

/** The compact top-level destination that owns this route (same as `topLevel(wide = false)`). */
fun Route.topLevel(): Route = topLevel(wide = false)

/**
 * The top-level destination that owns this route, used to highlight the nav item (12 §3.15).
 * Compact: Torrents -> Library; Extensions / Link / Linked / Settings / SettingsSection -> You.
 * Wide: every rail destination is its own (Torrents(*) -> Torrents(), Link -> Linked, SettingsSection / You -> Settings).
 * Routes without an owner (Details, Player) return themselves; the shell then highlights the entry below them in the back stack.
 */
fun Route.topLevel(wide: Boolean): Route = when (this) {
    is Route.SearchFor -> Route.Search
    is Route.LibraryTab -> Route.Library
    is Route.Torrents -> if (wide) Route.Torrents() else Route.Library
    Route.Extensions -> if (wide) Route.Extensions else Route.You
    Route.Link, Route.Linked -> if (wide) Route.Linked else Route.You
    Route.Settings, is Route.SettingsSection -> if (wide) Route.Settings else Route.You
    Route.You -> if (wide) Route.Settings else Route.You
    else -> this
}
