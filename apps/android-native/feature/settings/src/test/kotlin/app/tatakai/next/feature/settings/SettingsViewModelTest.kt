package app.tatakai.next.feature.settings

import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataCacheStats
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.model.ThemeMode
import app.tatakai.next.model.AppLanguage
import app.cash.turbine.test
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class SettingsViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeSettings : SettingsRepository {
        override val settings = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }


    @Test
    fun `appearance changes are written through the settings repository`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo)

        vm.setPalette("sakura")
        vm.setAmoled(true)
        vm.setDynamicColor(false)
        vm.setContrast(ContrastPref.HIGH)

        val s = repo.settings.value
        assertEquals("sakura", s.paletteId)
        assertEquals(true, s.amoled)
        assertEquals(false, s.dynamicColor)
        assertEquals(ContrastPref.HIGH, s.contrast)
        assertEquals("sakura", vm.state.first { it.settings.paletteId == "sakura" }.settings.paletteId)
    }

    @Test
    fun `provider priority moves and clamps`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo)

        vm.moveProvider("kitsu", -1)
        assertEquals(listOf("anilist", "kitsu", "anizip"), repo.settings.value.providerPriority)
        vm.moveProvider("anilist", -1)
        assertEquals(listOf("anilist", "kitsu", "anizip"), repo.settings.value.providerPriority)
        assertEquals(listOf("a", "b"), listOf("a", "b").moved("zzz", 1))
    }



    private class FakeMetadata(var stats: MetadataCacheStats = MetadataCacheStats(entries = 12, bytes = 98_000_000)) : MetadataRepository {
        var cleared = false
        var failClear = false
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage = error("unused")
        override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage = error("unused")
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? = null
        override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
        override suspend fun cacheStats(): MetadataCacheStats = stats
        override suspend fun clearCache() {
            if (failClear) throw AppException(AppError.Unknown("disk"))
            cleared = true
            stats = MetadataCacheStats(0, 0)
        }
    }

    @Test
    fun `wave settings are written through the repository`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo)

        vm.setThemeMode(ThemeMode.DARK)
        vm.setLanguage(AppLanguage.ES)
        vm.setReduceMotion(true)
        vm.setPlayerEngine(PlayerEngine.EXTERNAL)
        vm.setExternalPlayerPackage("is.xyz.mpv")
        vm.setPlayerPreset("anime4k-b")
        vm.setSourceMode(SourceMode.MANUAL)
        vm.setPreferredKind(SourceKind.TORRENT)
        vm.setMinQuality(Quality.P1080)
        vm.setSubLang("en")
        vm.setDubLang("es")
        vm.setAutoNext(false)
        vm.setSkipIntro(false)

        val s = repo.settings.value
        assertEquals(ThemeMode.DARK, s.themeMode)
        assertEquals(AppLanguage.ES, s.language)
        assertEquals(true, s.reduceMotion)
        assertEquals(PlayerEngine.EXTERNAL, s.playerEngine)
        assertEquals("is.xyz.mpv", s.externalPlayerPackage)
        assertEquals("anime4k-b", s.playerPreset)
        assertEquals(SourceMode.MANUAL, s.sourcePrefs.mode)
        assertEquals(SourceKind.TORRENT, s.sourcePrefs.prefer)
        assertEquals(Quality.P1080, s.sourcePrefs.minQuality)
        assertEquals("en", s.sourcePrefs.subLang)
        assertEquals("es", s.sourcePrefs.dubLang)
        assertEquals(false, s.autoNext)
        assertEquals(false, s.skipIntro)

        vm.setExternalPlayerPackage(" ")
        assertNull(repo.settings.value.externalPlayerPackage)
    }

    @Test
    fun `ttl and subtitle style are clamped`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo)

        vm.setMetadataTtl(1000)
        assertEquals(168, repo.settings.value.metadataCacheTtlHours)
        vm.setMetadataTtl(0)
        assertEquals(1, repo.settings.value.metadataCacheTtlHours)

        vm.setSubtitleStyle(SubtitleStyle(scale = 5f, backgroundOpacity = -1f, font = "space_grotesk"))
        val style = repo.settings.value.subtitleStyle
        assertEquals(2f, style.scale)
        assertEquals(0f, style.backgroundOpacity)
        assertEquals("space_grotesk", style.font)
    }

    @Test
    fun `switch changes announce settings saved`() = runTest {
        val vm = SettingsViewModel(FakeSettings())
        vm.events.test {
            vm.setAutoNext(false)
            assertEquals(SettingsEvent.Saved, awaitItem())
            vm.setPalette("sakura")
            vm.setMinQuality(Quality.P480)
            assertEquals(SettingsEvent.Saved, awaitItem())
        }
    }

    @Test
    fun `storage loads the cache figures and clearing the cache reports the freed bytes`() = runTest {
        val metadata = FakeMetadata()
        val vm = SettingsViewModel(FakeSettings(), metadata = metadata)

        vm.refreshStorage()
        val ok = vm.state.first { it.storage is LoadState.Ok }.storage as LoadState.Ok
        assertEquals(98_000_000L, ok.value.cache?.bytes)
        assertNull(ok.value.torrents)

        vm.events.test {
            vm.clearCache()
            assertEquals(SettingsEvent.CacheCleared(98_000_000), awaitItem())
        }
        assertEquals(true, metadata.cleared)
    }

    @Test
    fun `cache clear failure is reported`() = runTest {
        val metadata = FakeMetadata().apply { failClear = true }
        val vm = SettingsViewModel(FakeSettings(), metadata = metadata)
        vm.events.test {
            vm.clearCache()
            assertEquals(SettingsEvent.Failed(AppError.Unknown("disk")), awaitItem())
        }
    }

    private class FakeImages(var bytes: Long? = 40_000_000) : ImageCache {
        var cleared = false
        var failClear = false
        override suspend fun sizeBytes(): Long? = bytes
        override suspend fun clear() {
            if (failClear) throw AppException(AppError.Unknown("images"))
            cleared = true
            bytes = 0
        }
    }

    @Test
    fun `clearing the cache also empties the image disk cache and reports both as freed`() = runTest {
        val metadata = FakeMetadata()
        val images = FakeImages()
        val vm = SettingsViewModel(FakeSettings(), metadata = metadata, images = images)

        vm.refreshStorage()
        val ok = vm.state.first { it.storage is LoadState.Ok }.storage as LoadState.Ok
        assertEquals(40_000_000L, ok.value.imageBytes)
        assertEquals(138_000_000L, ok.value.clearableBytes)

        vm.events.test {
            vm.clearCache()
            assertEquals(SettingsEvent.CacheCleared(138_000_000), awaitItem())
        }
        assertEquals(true, metadata.cleared)
        assertEquals(true, images.cleared)
        val after = vm.state.first { (it.storage as? LoadState.Ok)?.value?.clearableBytes == 0L }
        assertEquals(false, after.clearingCache)
    }

    @Test
    fun `image cache is cleared even when the metadata clear fails`() = runTest {
        val metadata = FakeMetadata().apply { failClear = true }
        val images = FakeImages()
        val vm = SettingsViewModel(FakeSettings(), metadata = metadata, images = images)
        vm.events.test {
            vm.clearCache()
            assertEquals(SettingsEvent.Failed(AppError.Unknown("disk")), awaitItem())
        }
        assertEquals(true, images.cleared)
    }

    @Test
    fun `image cache alone can be cleared`() = runTest {
        val images = FakeImages(bytes = 5_000)
        val vm = SettingsViewModel(FakeSettings(), images = images)
        vm.events.test {
            vm.clearCache()
            assertEquals(SettingsEvent.CacheCleared(5_000), awaitItem())
        }
    }

    @Test
    fun `freed bytes never go negative and clearable bytes need one known figure`() {
        assertEquals(0L, freedBytes(before = 10, after = 25))
        assertEquals(15L, freedBytes(before = 25, after = 10))
        assertNull(StorageInfo(cache = null, torrents = null).clearableBytes)
        assertEquals(7L, StorageInfo(cache = null, torrents = null, imageBytes = 7).clearableBytes)
    }
}
