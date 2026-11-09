@file:OptIn(ExperimentalMaterial3AdaptiveApi::class)

package app.tatakai.next.nav

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import android.graphics.Color
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.ContentTransform
import androidx.compose.animation.EnterTransition
import androidx.compose.animation.ExitTransition
import androidx.compose.animation.expandVertically
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.Extension
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Person
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.VideoLibrary
import androidx.compose.material.icons.outlined.CloudOff
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Home
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.VideoLibrary
import androidx.compose.material3.Badge
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.navigation3.ListDetailSceneStrategy
import androidx.compose.material3.adaptive.navigation3.rememberListDetailSceneStrategy
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteItem
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteType
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.navigation3.rememberViewModelStoreNavEntryDecorator
import androidx.navigation3.runtime.NavBackStack
import androidx.navigation3.runtime.NavKey
import androidx.navigation3.runtime.entryProvider
import androidx.navigation3.runtime.rememberNavBackStack
import androidx.navigation3.runtime.rememberSaveableStateHolderNavEntryDecorator
import androidx.navigation3.ui.NavDisplay
import androidx.navigation3.ui.defaultPopTransitionSpec
import androidx.navigation3.ui.defaultPredictivePopTransitionSpec
import androidx.navigation3.ui.defaultTransitionSpec
import app.tatakai.next.AppLocale
import app.tatakai.next.R
import app.tatakai.next.designsystem.components.AppSnackbarController
import app.tatakai.next.designsystem.components.AppSnackbarHost
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.rememberAppSnackbarController
import app.tatakai.next.designsystem.theme.LocalReduceMotion
import app.tatakai.next.designsystem.theme.TatakaiTheme
import app.tatakai.next.designsystem.theme.rememberReduceMotion
import app.tatakai.next.feature.details.CharacterScreen
import app.tatakai.next.feature.details.DetailsScreen
import app.tatakai.next.feature.extensions.ExtensionsScreen
import app.tatakai.next.feature.home.HomeScreen
import app.tatakai.next.feature.library.LibraryScreen
import app.tatakai.next.feature.library.LibraryTab
import app.tatakai.next.feature.link.ExtensionServersScreen
import app.tatakai.next.feature.link.LinkedDeviceScreen
import app.tatakai.next.feature.player.PlayerScreen
import app.tatakai.next.feature.search.ImageSearchScreen
import app.tatakai.next.feature.search.SearchScreen
import app.tatakai.next.feature.settings.OnboardingScreen
import app.tatakai.next.feature.settings.SettingsScreen
import app.tatakai.next.feature.torrents.TorrentsScreen
import app.tatakai.next.feature.you.YouScreen
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.model.ThemeMode
import app.tatakai.next.model.UpdateRepository
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentSection
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.state.contrastLevel
import app.tatakai.next.ui.state.rememberIsOnline
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import org.koin.compose.getKoin
import org.koin.compose.koinInject
import app.tatakai.next.ui.R as UiR

/**
 * Root composable: theme from the persisted settings (palette, AMOLED, contrast, dynamic color, theme mode) + per-app language +
 * the app-wide CompositionLocals (LocalReduceMotion, LocalAppSnackbar) + adaptive shell + Navigation3 back stack (12 §3.15, §4 b.1).
 */
@Composable
fun TatakaiApp(
    torrentLinks: MutableStateFlow<TorrentLink?>? = null,
    serverLinks: MutableStateFlow<Boolean>? = null,
    // parity wave A (15 §3.6): OAuth redirect arrived (open Settings › Listas y cuentas) / an image was shared to the app (image search).
    accountLinks: MutableStateFlow<Boolean>? = null,
    imageLinks: MutableStateFlow<String?>? = null,
) {
    val settingsRepo = koinInject<SettingsRepository>()
    val settings by settingsRepo.settings.collectAsStateWithLifecycle()
    val dark = when (settings.themeMode) {
        ThemeMode.SYSTEM -> isSystemInDarkTheme()
        ThemeMode.LIGHT -> false
        ThemeMode.DARK -> true
    }
    SystemBarsFollowTheme(dark)
    PerAppLanguage(settings, settingsRepo)

    TatakaiTheme(
        paletteId = settings.paletteId,
        amoled = settings.amoled,
        contrast = settings.contrastLevel(),
        dynamicColor = settings.dynamicColor,
        darkTheme = dark,
    ) {
        val reduceMotion = rememberReduceMotion(settings.reduceMotion)
        val snackbar = rememberAppSnackbarController()
        CompositionLocalProvider(
            LocalReduceMotion provides reduceMotion,
            LocalAppSnackbar provides snackbar,
            LocalTitleLanguage provides settings.titleLanguage,
        ) {
            // parity wave A, 2026-11-09 amendment (plan 14 §0.1 #11, worker q): first-run wizard until `AppSettings.onboardingDone`;
            // the wizard's ViewModel persists the flag (and "Restablecer la app" clears it), so this just reacts to the settings.
            if (!settings.onboardingDone) {
                OnboardingScreen(onDone = {})
            } else {
                AppShell(snackbar, torrentLinks, serverLinks, accountLinks, imageLinks)
            }
        }
    }
}

/** Status / navigation bar icons follow the in-app theme mode, not only the system one. */
@Composable
private fun SystemBarsFollowTheme(dark: Boolean) {
    val activity = LocalContext.current.findActivity() as? ComponentActivity ?: return
    DisposableEffect(activity, dark) {
        activity.enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT) { dark },
            navigationBarStyle = SystemBarStyle.auto(Color.TRANSPARENT, Color.TRANSPARENT) { dark },
        )
        onDispose { }
    }
}

/**
 * Settings › Idioma -> LocaleManager (33+) / recreate with [AppLocale.wrap] (< 33). On the first composition a language chosen in the
 * system per-app language screen (33+) wins and is copied into the settings, so the two never fight.
 */
@Composable
private fun PerAppLanguage(settings: AppSettings, repo: SettingsRepository) {
    val activity = LocalContext.current.findActivity() ?: return
    // Not an effect key: flipping it must not cancel the running effect (it may be inside repo.setLanguage).
    var adopted by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(settings.language) {
        if (!adopted) {
            adopted = true
            val system = AppLocale.systemChoice(activity)
            if (system != null && system != settings.language) {
                repo.setLanguage(system)
                return@LaunchedEffect
            }
        }
        AppLocale.apply(activity, settings.language)
    }
}

@Composable
private fun AppShell(
    snackbar: AppSnackbarController,
    torrentLinks: MutableStateFlow<TorrentLink?>? = null,
    serverLinks: MutableStateFlow<Boolean>? = null,
    accountLinks: MutableStateFlow<Boolean>? = null,
    imageLinks: MutableStateFlow<String?>? = null,
) {
    val backStack = rememberNavBackStack(Route.Home)
    val current = backStack.lastOrNull() as? Route

    // parity wave A (15 §3.3): tatakai://oauth/<provider> came back from the Custom Tab. The redirect itself waits in PendingOAuthRedirect
    // (worker q's ListsRepository finishes the exchange); the shell only shows the Accounts section so the user sees the result.
    if (accountLinks != null) {
        LaunchedEffect(accountLinks) {
            accountLinks.collect { open ->
                if (open) {
                    val target = Route.SettingsSection(SettingsSection.ACCOUNTS)
                    if (backStack.lastOrNull() != target) backStack.add(target)
                    accountLinks.value = false
                }
            }
        }
    }

    // parity wave A (15 §3.1): an image shared to the app opens the image search with it.
    if (imageLinks != null) {
        LaunchedEffect(imageLinks) {
            imageLinks.collect { uri ->
                if (uri != null) {
                    val target = Route.ImageSearch(uri)
                    if (backStack.lastOrNull() != target) backStack.add(target)
                    imageLinks.value = null
                }
            }
        }
    }

    // Torrent notification taps (integration of f's notification extras): open the client section, Back returns to where we were.
    if (torrentLinks != null) {
        LaunchedEffect(torrentLinks) {
            torrentLinks.collect { link ->
                if (link != null) {
                    val target = Route.Torrents(link.section, link.torrentId)
                    if (backStack.lastOrNull() != target) backStack.add(target)
                    torrentLinks.value = null
                }
            }
        }
    }

    // tatakai://extensionserver/connect (docs/redesign/13 §3.10): MainActivity left the link in PendingServerLink; open the screen,
    // which asks the user before pairing.
    if (serverLinks != null) {
        LaunchedEffect(serverLinks) {
            serverLinks.collect { open ->
                if (open) {
                    if (backStack.lastOrNull() != Route.ExtensionServers) backStack.add(Route.ExtensionServers)
                    serverLinks.value = false
                }
            }
        }
    }

    // ShortNavigationBar on compact windows (< 600 dp), WideNavigationRail from medium width; hidden over the player.
    val wide = LocalConfiguration.current.screenWidthDp >= WIDE_MIN_WIDTH_DP
    val destinations = topLevelRoutes(wide)
    val selectedTop = backStack.asReversed()
        .firstNotNullOfOrNull { key -> (key as? Route)?.topLevel(wide)?.takeIf { it in destinations } }
    val onPlayer = current is Route.Player
    val suiteType = when {
        onPlayer -> NavigationSuiteType.None
        wide -> NavigationSuiteType.WideNavigationRailCollapsed
        else -> NavigationSuiteType.ShortNavigationBarCompact
    }

    val torrentStatus by rememberTorrentStatus().collectAsStateWithLifecycle()
    val activeDownloads = torrentStatus.activeDownloads
    val online = rememberIsOnline()
    val showOffline = !online && !onPlayer

    // parity wave A (15 §3.4, checklist system.update-startup-notice): one snackbar per newer release, "Ver" opens Acerca de.
    // Worker q's UpdateRepository publishes it (the contract stub never does); AppSettings.updateCheck is honoured inside the repository.
    val updates = koinInject<UpdateRepository>()
    val latestUpdate by updates.latest.collectAsStateWithLifecycle()
    val updateMessage = latestUpdate?.let { stringResource(R.string.app_update_available, it.version) }
    val updateAction = stringResource(R.string.app_update_view)
    LaunchedEffect(latestUpdate?.version) {
        if (updateMessage != null) {
            snackbar.show(updateMessage, updateAction) { backStack.add(Route.SettingsSection(SettingsSection.ABOUT)) }
        }
    }

    NavigationSuiteScaffold(
        navigationItems = {
            val badgeDescription = pluralStringResource(R.plurals.app_active_downloads, activeDownloads, activeDownloads)
            destinations.forEach { route ->
                val selected = route == selectedTop
                // Badge = active downloads: on Library (compact, Torrents is a Library tab) / on Torrents (rail).
                val badged = activeDownloads > 0 &&
                    ((!wide && route == Route.Library) || (wide && route is Route.Torrents))
                NavigationSuiteItem(
                    selected = selected,
                    onClick = { backStack.navigateTop(route) },
                    icon = { Icon(route.icon(selected), contentDescription = null) },
                    label = { Text(stringResource(route.labelRes())) },
                    navigationSuiteType = suiteType,
                    badge = if (badged) {
                        {
                            Badge(Modifier.semantics { contentDescription = badgeDescription }) {
                                Text(if (activeDownloads > 99) "99+" else activeDownloads.toString())
                            }
                        }
                    } else {
                        null
                    },
                )
            }
        },
        navigationSuiteType = suiteType,
        modifier = Modifier.fillMaxSize(),
    ) {
        // The rail does not consume the bottom system inset (the bar does): the banner and snackbar add it themselves there.
        val bottomInsets = if (suiteType == NavigationSuiteType.ShortNavigationBarCompact) WindowInsets(0) else WindowInsets.navigationBars
        Box(Modifier.fillMaxSize()) {
            Column(Modifier.fillMaxSize()) {
                Box(Modifier.weight(1f).fillMaxWidth()) {
                    AppNavDisplay(backStack, wide)
                }
                AnimatedVisibility(visible = showOffline, enter = expandVertically(), exit = shrinkVertically()) {
                    OfflineBanner(Modifier.windowInsetsPadding(bottomInsets))
                }
            }
            AppSnackbarHost(
                snackbar,
                Modifier
                    .align(Alignment.BottomCenter)
                    .windowInsetsPadding(bottomInsets)
                    .padding(bottom = if (showOffline) OFFLINE_BANNER_HEIGHT else 0.dp),
            )
        }
    }
}

/** Slim persistent banner while the default network is not validated (prototype `offline.banner`). */
@Composable
private fun OfflineBanner(modifier: Modifier = Modifier) {
    Surface(
        color = MaterialTheme.colorScheme.tertiaryContainer,
        contentColor = MaterialTheme.colorScheme.onTertiaryContainer,
        modifier = modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
    ) {
        Row(
            Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(Icons.Outlined.CloudOff, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(stringResource(UiR.string.ui_offline_banner), style = MaterialTheme.typography.labelLarge)
        }
    }
}

/**
 * Torrent service status for the download badge. Resolved lazily from Koin so the shell keeps working when the torrent engine is
 * not bound (previews, a build without :core:torrent wired).
 */
@Composable
private fun rememberTorrentStatus(): StateFlow<TorrentServiceStatus> {
    val koin = getKoin()
    return remember(koin) {
        runCatching { koin.getOrNull<TorrentRepository>() }.getOrNull()?.status ?: MutableStateFlow(TorrentServiceStatus())
    }
}

@Composable
private fun AppNavDisplay(backStack: NavBackStack<NavKey>, wide: Boolean) {
    // List-detail on expanded windows: Search/Library are list panes, Details the detail pane.
    val listDetail = rememberListDetailSceneStrategy<NavKey>()

    fun openDetails(anilistId: Int) {
        if (backStack.lastOrNull() is Route.Details) backStack.removeLastOrNull()
        backStack.add(Route.Details(MediaKind.ANIME, anilistId))
    }
    fun push(route: Route) {
        if (backStack.lastOrNull() != route) backStack.add(route)
    }
    fun replaceTop(route: Route) {
        backStack.removeLastOrNull()
        backStack.add(route)
    }
    fun play(args: PlayerArgs) = push(Route.Player(args))
    fun back() {
        if (backStack.size > 1) backStack.removeLastOrNull()
    }
    fun openTorrent(torrentId: String) = push(Route.Torrents(TorrentSection.OVERVIEW, torrentId))
    fun openSettings(section: SettingsSection) = push(Route.SettingsSection(section))
    fun openGenre(genre: String) = push(Route.SearchFor(genre = genre))

    // Reduce motion: instant screen changes (no slide / fade), predictive back included.
    val reduceMotion = LocalReduceMotion.current
    val instant = remember { ContentTransform(EnterTransition.None, ExitTransition.None) }

    NavDisplay(
        backStack = backStack,
        onBack = ::back,
        sceneStrategy = listDetail,
        transitionSpec = if (reduceMotion) ({ instant }) else defaultTransitionSpec(),
        popTransitionSpec = if (reduceMotion) ({ instant }) else defaultPopTransitionSpec(),
        predictivePopTransitionSpec = if (reduceMotion) ({ _ -> instant }) else defaultPredictivePopTransitionSpec(),
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
                    onOpenFeed = { feed -> push(Route.SearchFor(feed = feed)) },
                    onOpenGenre = ::openGenre,
                    onSearchQuery = { query -> push(Route.SearchFor(query = query)) },
                    onOpenLibrary = { backStack.navigateTop(Route.Library) },
                )
            }
            entry<Route.Search>(metadata = ListDetailSceneStrategy.listPane()) {
                SearchScreen(
                    onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } },
                    onOpenImageSearch = { push(Route.ImageSearch()) },
                )
            }
            entry<Route.SearchFor>(metadata = ListDetailSceneStrategy.listPane()) { key ->
                SearchScreen(
                    onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } },
                    initialQuery = key.query,
                    initialFeed = key.feed,
                    initialGenre = key.genre,
                    initialStudio = key.studio,
                    onOpenImageSearch = { push(Route.ImageSearch()) },
                )
            }
            // parity wave A (15 §3.1, worker o): image search (trace.moe), opened from the search bar or by sharing a picture.
            entry<Route.ImageSearch>(metadata = ListDetailSceneStrategy.listPane()) { key ->
                ImageSearchScreen(
                    onBack = ::back,
                    onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } },
                    imageUri = key.imageUri,
                )
            }
            entry<Route.Library>(metadata = ListDetailSceneStrategy.listPane()) {
                LibraryScreen(
                    onOpenDetails = ::openDetails,
                    onPlay = ::play,
                    onExplore = { backStack.navigateTop(Route.Home) },
                    onOpenTorrent = ::openTorrent,
                    onOpenAccounts = { openSettings(SettingsSection.ACCOUNTS) },
                )
            }
            entry<Route.LibraryTab>(metadata = ListDetailSceneStrategy.listPane()) { key ->
                LibraryScreen(
                    onOpenDetails = ::openDetails,
                    onPlay = ::play,
                    onExplore = { backStack.navigateTop(Route.Home) },
                    initialTab = key.tab,
                    onOpenTorrent = ::openTorrent,
                    onOpenAccounts = { openSettings(SettingsSection.ACCOUNTS) },
                )
            }
            entry<Route.Details>(metadata = ListDetailSceneStrategy.detailPane()) { key ->
                DetailsScreen(
                    kind = key.kind,
                    anilistId = key.anilistId,
                    onBack = ::back,
                    onPlay = ::play,
                    onOpenExtensions = { push(Route.Extensions) },
                    onOpenTorrent = ::openTorrent,
                    onOpenSettings = ::openSettings,
                    onOpenLinked = { push(Route.Linked) },
                    onOpenDownloads = { push(Route.Torrents(TorrentSection.DOWNLOADS)) },
                    onOpenGenre = ::openGenre,
                    onOpenCharacter = { id -> push(Route.Character(id)) },
                    onOpenStudio = { studio -> push(Route.SearchFor(studio = studio)) },
                )
            }
            // parity wave A (15 §3.1, worker o): character page, detail pane like Details.
            entry<Route.Character>(metadata = ListDetailSceneStrategy.detailPane()) { key ->
                CharacterScreen(
                    characterId = key.characterId,
                    onBack = ::back,
                    onMediaClick = { media -> media.ref.anilistId?.let { openDetails(it) } },
                )
            }
            entry<Route.Player> { key ->
                PlayerScreen(
                    args = key.args,
                    onBack = ::back,
                    onReplace = { args -> replaceTop(Route.Player(args)) },
                    onOpenTorrent = ::openTorrent,
                    onOpenSettings = ::openSettings,
                    onOpenExtensions = { push(Route.Extensions) },
                    onOpenLinked = { push(Route.Linked) },
                )
            }
            entry<Route.You> {
                YouScreen(
                    onOpenHistory = { push(Route.LibraryTab(LibraryTab.HISTORY)) },
                    onOpenDownloads = { push(Route.Torrents(TorrentSection.DOWNLOADS)) },
                    onOpenExtensions = { push(Route.Extensions) },
                    onOpenLinked = { push(Route.Linked) },
                    onOpenSettings = { push(Route.Settings) },
                    onOpenAccounts = { openSettings(SettingsSection.ACCOUNTS) },
                )
            }
            entry<Route.Settings> {
                SettingsScreen(
                    onOpenExtensions = { push(Route.Extensions) },
                    onOpenLink = { push(Route.Linked) },
                    onOpenLinked = { push(Route.Linked) },
                    // Rail destination on wide windows (no back arrow); reached from "Tú" on phones.
                    onBack = if (wide) null else ::back,
                )
            }
            entry<Route.SettingsSection> { key ->
                SettingsScreen(
                    onOpenExtensions = { push(Route.Extensions) },
                    onOpenLink = { push(Route.Linked) },
                    initialSection = key.section,
                    onOpenLinked = { push(Route.Linked) },
                    onBack = ::back,
                )
            }
            entry<Route.Extensions> { ExtensionsScreen(onBack = ::back) }
            entry<Route.Link> {
                LinkedDeviceScreen(onBack = ::back, onPlay = ::play, onOpenExtensionServers = { push(Route.ExtensionServers) })
            }
            entry<Route.Linked> {
                LinkedDeviceScreen(onBack = ::back, onPlay = ::play, onOpenExtensionServers = { push(Route.ExtensionServers) })
            }
            entry<Route.ExtensionServers> { ExtensionServersScreen(onBack = ::back) }
            entry<Route.Torrents> { key ->
                TorrentsScreen(
                    section = key.section,
                    torrentId = key.torrentId,
                    onNavigate = { section, torrentId -> replaceTop(Route.Torrents(section, torrentId)) },
                    onBack = ::back,
                    onPlay = ::play,
                )
            }
        },
    )
}

/** Switching tabs resets the stack to `[Home]` or `[Home, tab]` so Back from a tab returns to Home. */
private fun NavBackStack<NavKey>.navigateTop(route: Route) {
    val target = if (route == Route.Home) listOf<NavKey>(Route.Home) else listOf(Route.Home, route)
    if (toList() == target) return
    clear()
    addAll(target)
}

private const val WIDE_MIN_WIDTH_DP = 600
private val OFFLINE_BANNER_HEIGHT = 40.dp

private fun Route.icon(selected: Boolean): ImageVector = when (this) {
    Route.Home -> if (selected) Icons.Filled.Home else Icons.Outlined.Home
    Route.Search -> if (selected) Icons.Filled.Search else Icons.Outlined.Search
    Route.Library -> if (selected) Icons.Filled.VideoLibrary else Icons.Outlined.VideoLibrary
    is Route.Torrents -> if (selected) Icons.Filled.Download else Icons.Outlined.Download
    Route.Extensions -> if (selected) Icons.Filled.Extension else Icons.Outlined.Extension
    Route.Linked, Route.Link -> if (selected) Icons.Filled.Computer else Icons.Outlined.Computer
    Route.Settings -> if (selected) Icons.Filled.Settings else Icons.Outlined.Settings
    else -> if (selected) Icons.Filled.Person else Icons.Outlined.Person
}

private fun Route.labelRes(): Int = when (this) {
    Route.Home -> R.string.nav_home
    Route.Search -> R.string.nav_search
    Route.Library -> R.string.nav_library
    is Route.Torrents -> UiR.string.nav_torrents
    Route.Extensions -> R.string.nav_extensions
    Route.Linked, Route.Link -> R.string.nav_linked
    Route.Settings -> R.string.nav_settings
    else -> R.string.nav_you
}

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
