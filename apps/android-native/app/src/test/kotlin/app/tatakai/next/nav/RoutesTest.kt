package app.tatakai.next.nav

import app.tatakai.next.feature.library.LibraryTab
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.model.torrent.TorrentSection
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class RoutesTest {

    @Test
    fun `compact has four destinations and wide has eleven in rail order`() {
        assertEquals(listOf(Route.Home, Route.Search, Route.Library, Route.You), topLevelRoutes(wide = false))
        // parity wave B (16 §3.12): Manga, Novelas, Descargas and Perfil join the rail.
        assertEquals(
            listOf(
                Route.Home, Route.Search, Route.Library, Route.Manga(), Route.Novels(), Route.Downloads(), Route.Torrents(),
                Route.Extensions, Route.Linked, Route.Profile(), Route.Settings,
            ),
            topLevelRoutes(wide = true),
        )
    }

    @Test
    fun `wave B routes fold into their rail destination on wide and into home or you on compact`() {
        assertEquals(Route.Manga(), Route.Manga("frieren").topLevel(wide = true))
        assertEquals(Route.Home, Route.Manga("frieren").topLevel(wide = false))
        assertEquals(Route.Novels(), Route.NovelDetails(1).topLevel(wide = true))
        assertEquals(Route.Downloads(), Route.Downloads(app.tatakai.next.model.DownloadsTab.HISTORY).topLevel(wide = true))
        assertEquals(Route.You, Route.Downloads().topLevel(wide = false))
        assertEquals(Route.Profile(), Route.Notifications.topLevel(wide = true))
        assertEquals(Route.You, Route.Profile("someone").topLevel(wide = false))
        assertEquals(Route.Extensions, Route.Provider("ext", "src").topLevel(wide = true))
        assertEquals(Route.Home, Route.Calendar.topLevel(wide = false))
    }

    @Test
    fun `compact folds torrents into library and settings-like routes into you`() {
        assertEquals(Route.Library, Route.Torrents(TorrentSection.PEERS, "abc").topLevel(wide = false))
        assertEquals(Route.Library, Route.LibraryTab(LibraryTab.HISTORY).topLevel(wide = false))
        listOf(Route.Extensions, Route.Link, Route.Linked, Route.Settings, Route.SettingsSection(SettingsSection.PLAYER), Route.You)
            .forEach { assertEquals(Route.You, it.topLevel(wide = false), "$it") }
        assertEquals(Route.Search, Route.SearchFor(feed = FeedKind.entries.first()).topLevel(wide = false))
        assertEquals(Route.You, Route.Extensions.topLevel())
        // parity wave A (15 §3.6)
        assertEquals(Route.Search, Route.SearchFor(studio = "MAPPA").topLevel(wide = false))
        assertEquals(Route.Search, Route.ImageSearch().topLevel(wide = false))
        assertEquals(Route.Search, Route.ImageSearch("content://x").topLevel(wide = true))
    }

    @Test
    fun `wide maps every rail destination to itself`() {
        assertEquals(Route.Torrents(), Route.Torrents(TorrentSection.FILES, "x").topLevel(wide = true))
        assertEquals(Route.Extensions, Route.Extensions.topLevel(wide = true))
        assertEquals(Route.Linked, Route.Link.topLevel(wide = true))
        assertEquals(Route.Settings, Route.SettingsSection(SettingsSection.TORRENTS).topLevel(wide = true))
        topLevelRoutes(wide = true).forEach { assertEquals(it, it.topLevel(wide = true)) }
    }

    @Test
    fun `routes without an owner return themselves`() {
        val details = Route.Details(MediaKind.ANIME, 1)
        assertEquals(details, details.topLevel(wide = true))
        assertTrue(details.topLevel(wide = false) !in topLevelRoutes(wide = false))
    }
}
