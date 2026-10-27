@file:OptIn(ExperimentalMaterial3AdaptiveApi::class)

package app.tatakai.next.nav

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.VideoLibrary
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.VideoLibrary
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.navigation3.ListDetailSceneStrategy
import androidx.compose.material3.adaptive.navigation3.rememberListDetailSceneStrategy
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteItem
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteType
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import androidx.navigation3.ui.NavDisplay
import app.tatakai.next.R
import app.tatakai.next.designsystem.theme.TatakaiTheme
import app.tatakai.next.feature.details.DetailsScreen
import app.tatakai.next.feature.extensions.ExtensionsScreen
import app.tatakai.next.feature.home.HomeScreen
import app.tatakai.next.feature.library.LibraryScreen
import app.tatakai.next.feature.link.LinkScreen
import app.tatakai.next.feature.player.PlayerScreen
import app.tatakai.next.feature.search.SearchScreen
import app.tatakai.next.feature.settings.SettingsScreen
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.ui.state.contrastLevel
import org.koin.compose.koinInject

/** Root composable: theme from the persisted settings + adaptive shell + Navigation3 back stack. */
@Composable
fun TatakaiApp() {
    val settingsRepo = koinInject<SettingsRepository>()
    val settings by settingsRepo.settings.collectAsStateWithLifecycle()
    TatakaiTheme(
        paletteId = settings.paletteId,
        amoled = settings.amoled,
        contrast = settings.contrastLevel(),
        dynamicColor = settings.dynamicColor,
    ) {
        AppShell()
    }
}

@Composable
private fun AppShell() {
    val backStack = rememberNavBackStack(Route.Home)
    val current = backStack.lastOrNull() as? Route
    val selectedTop = backStack.lastOrNull { (it as? Route)?.topLevel() in TopLevelRoutes }
        ?.let { (it as Route).topLevel() }

    // ShortNavigationBar on compact windows, WideNavigationRail from medium width; hidden over the player.
    val wide = LocalConfiguration.current.screenWidthDp >= WIDE_MIN_WIDTH_DP
    val suiteType = when {
        current is Route.Player -> NavigationSuiteType.None
        wide -> NavigationSuiteType.WideNavigationRailCollapsed
        else -> NavigationSuiteType.ShortNavigationBarCompact
    }

    NavigationSuiteScaffold(
        navigationItems = {
            TopLevelRoutes.forEach { route ->
                val selected = route == selectedTop
                NavigationSuiteItem(
                    selected = selected,
                    onClick = { backStack.navigateTop(route) },
                    icon = { Icon(route.icon(selected), contentDescription = null) },
                    label = { Text(stringResource(route.labelRes())) },
                    navigationSuiteType = suiteType,
                )
            }
        },
        navigationSuiteType = suiteType,
        modifier = Modifier.fillMaxSize(),
    ) {
        AppNavDisplay(backStack)
    }
}

@Composable
private fun AppNavDisplay(backStack: NavBackStack<NavKey>) {
    // List-detail on expanded windows: Search/Library are list panes, Details the detail pane.
    val listDetail = rememberListDetailSceneStrategy<NavKey>()

    fun openDetails(anilistId: Int) {
        if (backStack.lastOrNull() is Route.Details) backStack.removeLastOrNull()
        backStack.add(Route.Details(MediaKind.ANIME, anilistId))
    }
    fun play(args: PlayerArgs) {
        backStack.add(Route.Player(args))
    }
    fun back() {
        if (backStack.size > 1) backStack.removeLastOrNull()
    }

    NavDisplay(
        backStack = backStack,
        onBack = ::back,
        sceneStrategy = listDetail,
        entryDecorators = listOf(
            rememberSaveableStateHolderNavEntryDecorator(),
            rememberViewModelStoreNavEntryDecorator(),
        ),
        modifier = Modifier.fillMaxSize(),
        entryProvider = entryProvider {
            entry<Route.Home> {
                HomeScreen(
                    onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } },
                    onSearchClick = { backStack.navigateTop(Route.Search) },
                    onPlay = ::play,
                )
            }
            entry<Route.Search>(metadata = ListDetailSceneStrategy.listPane()) {
                SearchScreen(onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } })
            }
            entry<Route.Library>(metadata = ListDetailSceneStrategy.listPane()) {
                LibraryScreen(
                    onOpenDetails = ::openDetails,
                    onPlay = ::play,
                    onExplore = { backStack.navigateTop(Route.Home) },
                )
            }
            entry<Route.Details>(metadata = ListDetailSceneStrategy.detailPane()) { key ->
                DetailsScreen(
                    kind = key.kind,
                    anilistId = key.anilistId,
                    onBack = ::back,
                    onPlay = ::play,
                    onOpenExtensions = { backStack.add(Route.Extensions) },
                )
            }
            entry<Route.Player> { key ->
                PlayerScreen(args = key.args, onBack = ::back)
            }
            entry<Route.Settings> {
                SettingsScreen(
                    onOpenExtensions = { backStack.add(Route.Extensions) },
                    onOpenLink = { backStack.add(Route.Link) },
                )
            }
            entry<Route.Extensions> { ExtensionsScreen(onBack = ::back) }
            entry<Route.Link> { LinkScreen(onBack = ::back, onPlay = ::play) }
        },
    )
}

/** Switching tabs resets the stack to `[Home]` or `[Home, tab]` so Back from a tab returns to Home. */
private fun NavBackStack<NavKey>.navigateTop(route: Route) {
    if (lastOrNull() == route) return
    clear()
    add(Route.Home)
    if (route != Route.Home) add(route)
}

private const val WIDE_MIN_WIDTH_DP = 600

private fun Route.icon(selected: Boolean): ImageVector = when (this) {
    Route.Home -> if (selected) Icons.Filled.Home else Icons.Outlined.Home
    Route.Search -> if (selected) Icons.Filled.Search else Icons.Outlined.Search
    Route.Library -> if (selected) Icons.Filled.VideoLibrary else Icons.Outlined.VideoLibrary
    else -> if (selected) Icons.Filled.Person else Icons.Outlined.Person
}

private fun Route.labelRes(): Int = when (this) {
    Route.Home -> R.string.nav_home
    Route.Search -> R.string.nav_search
    Route.Library -> R.string.nav_library
    else -> R.string.nav_you
}
