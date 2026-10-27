package app.tatakai.next.nav

import androidx.navigation3.runtime.NavKey
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.PlayerArgs
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
}

/** Destinations shown in the navigation bar / rail. Extensions and Link live under Settings ("You"). */
val TopLevelRoutes: List<Route> = listOf(Route.Home, Route.Search, Route.Library, Route.Settings)

/** The top-level destination that owns [route] (used to highlight the right nav item). */
fun Route.topLevel(): Route = when (this) {
    Route.Extensions, Route.Link -> Route.Settings
    else -> this
}
