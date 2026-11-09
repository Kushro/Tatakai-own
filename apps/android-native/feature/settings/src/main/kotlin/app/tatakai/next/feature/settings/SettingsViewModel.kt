package app.tatakai.next.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.AppReset
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MetadataCacheStats
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.model.ThemeMode
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentStorage
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * Data & storage figures (Settings › Datos y almacenamiento): metadata cache, image disk cache ([imageBytes], Coil) and torrent storage.
 * Any of them may be unknown.
 */
data class StorageInfo(val cache: MetadataCacheStats?, val torrents: TorrentStorage?, val imageBytes: Long? = null) {
    /** What "Limpiar caché" frees: metadata table + images on disk; null when neither is known. */
    val clearableBytes: Long?
        get() = if (cache == null && imageBytes == null) null else (cache?.bytes ?: 0L) + (imageBytes ?: 0L)
}

/**
 * 2026-11-09 amendment (plan 14 §0.1 #9): no own login. The identity shown in Settings is the list of linked AniList / MAL accounts
 * ([accounts], `ListsRepository`); the history sync figures ([lastSyncedAt], [syncing], [linkPaired]) describe Tatakai Link.
 */
data class SettingsUiState(
    val settings: AppSettings = AppSettings(),
    // --- android wave (12 §4 b.4) ---
    val lastSyncedAt: Long? = null,
    val syncing: Boolean = false,
    val storage: LoadState<StorageInfo> = LoadState.Loading,
    val clearingCache: Boolean = false,
    // --- parity wave A (15 §3.3 / §3.4, worker q) ---
    /** Linked list providers (empty = local lists only). Worker q adds the OAuth / sync / conflict state it needs beside this. */
    val accounts: List<LinkedAccount> = emptyList(),
    /** A PC is paired over Tatakai Link (history sync possible). */
    val linkPaired: Boolean = false,
    val resetting: Boolean = false,
)

sealed interface SettingsEvent {
    /** Prototype `snack(t("settings.saved"))` after a change. */
    data object Saved : SettingsEvent
    data class CacheCleared(val freedBytes: Long) : SettingsEvent
    data class Failed(val cause: AppError) : SettingsEvent
    data object Synced : SettingsEvent

    /** parity wave A (worker q): "Restablecer la app" finished; the shell recreates the activity. */
    data object ResetDone : SettingsEvent
}

/**
 * Settings as one sectioned screen. Every field saves immediately through [SettingsRepository]. [history], [metadata], [torrents],
 * [lists], [link] and [reset] are optional so the existing tests (and a build without a module) keep working.
 */
class SettingsViewModel(
    private val settingsRepo: SettingsRepository,
    private val history: HistoryRepository? = null,
    private val metadata: MetadataRepository? = null,
    private val torrents: TorrentRepository? = null,
    // parity wave A (15 §3.3-3.4, worker q)
    private val lists: ListsRepository? = null,
    private val link: LinkRepository? = null,
    private val reset: AppReset? = null,
    // parity wave A closure (worker q, checklist system.clear-cache): Coil's disk cache of covers / banners
    private val images: ImageCache? = null,
) : ViewModel() {

    private val extras = MutableStateFlow(Extras())
    private val _events = Channel<SettingsEvent>(Channel.BUFFERED)
    val events: Flow<SettingsEvent> = _events.receiveAsFlow()

    val state: StateFlow<SettingsUiState> = combine(
        settingsRepo.settings,
        history?.lastSyncedAt ?: flowOf(null),
        lists?.accounts ?: flowOf(emptyList()),
        link?.state?.map { it is LinkState.Paired } ?: flowOf(false),
        extras,
    ) { s, synced, accounts, paired, x ->
        SettingsUiState(
            settings = s,
            lastSyncedAt = synced,
            syncing = x.syncing,
            storage = x.storage,
            clearingCache = x.clearingCache,
            accounts = accounts,
            linkPaired = paired,
            resetting = x.resetting,
        )
    }.stateIn(
        viewModelScope,
        SharingStarted.WhileSubscribed(5_000),
        SettingsUiState(settings = settingsRepo.settings.value),
    )

    // ---------------------------------------------------------------- appearance

    fun setPalette(id: String) = update { it.copy(paletteId = id) }

    fun setAmoled(value: Boolean) = update(announce = true) { it.copy(amoled = value) }

    fun setDynamicColor(value: Boolean) = update(announce = true) { it.copy(dynamicColor = value) }

    fun setContrast(value: ContrastPref) = update { it.copy(contrast = value) }

    fun setThemeMode(value: ThemeMode) = update { it.copy(themeMode = value) }

    fun setLanguage(value: AppLanguage) = update { it.copy(language = value) }

    fun setReduceMotion(value: Boolean) = update(announce = true) { it.copy(reduceMotion = value) }

    // ---------------------------------------------------------------- player

    fun setPlayerEngine(value: PlayerEngine) = update(announce = true) { it.copy(playerEngine = value) }

    /** null = system chooser. */
    fun setExternalPlayerPackage(value: String?) = update(announce = true) { it.copy(externalPlayerPackage = value?.takeIf(String::isNotBlank)) }

    fun setPlayerPreset(value: String) = update(announce = true) { it.copy(playerPreset = value) }

    fun setSourceMode(value: SourceMode) = updatePrefs { it.copy(mode = value) }

    /** null = any kind. */
    fun setPreferredKind(value: SourceKind?) = updatePrefs { it.copy(prefer = value) }

    fun setMinQuality(value: Quality) = updatePrefs { it.copy(minQuality = value) }

    fun setSubLang(value: String) = updatePrefs { it.copy(subLang = value) }

    fun setDubLang(value: String) = updatePrefs { it.copy(dubLang = value) }

    fun setAutoNext(value: Boolean) = update(announce = true) { it.copy(autoNext = value) }

    fun setSkipIntro(value: Boolean) = update(announce = true) { it.copy(skipIntro = value) }

    fun setSubtitleStyle(value: SubtitleStyle) = update {
        it.copy(subtitleStyle = value.copy(scale = value.scale.coerceIn(0.5f, 2f), backgroundOpacity = value.backgroundOpacity.coerceIn(0f, 1f)))
    }

    // ---------------------------------------------------------------- metadata

    /** Moves [providerId] by [delta] positions in the metadata priority list (negative = earlier). */
    fun moveProvider(providerId: String, delta: Int) = update { it.copy(providerPriority = it.providerPriority.moved(providerId, delta)) }

    fun setMetadataTtl(hours: Int) = update(announce = true) { it.copy(metadataCacheTtlHours = hours.coerceIn(1, 168)) }

    // ---------------------------------------------------------------- history over Tatakai Link (Enlace section)

    /** "Sincronizar ahora": push dirty rows to the paired PC and pull its recent ones. */
    fun syncNow() {
        val repo = history ?: return
        if (extras.value.syncing) return
        extras.update { it.copy(syncing = true) }
        viewModelScope.launch {
            val result = runCatchingApp { repo.syncNow().getOrThrow() }
            extras.update { it.copy(syncing = false) }
            _events.trySend(result.fold(onSuccess = { SettingsEvent.Synced }, onFailure = { SettingsEvent.Failed(it.toAppError()) }))
        }
    }

    // ---------------------------------------------------------------- data & storage

    /** Loads (or reloads) the cache and torrent storage figures. A failure of one source only blanks that figure. */
    fun refreshStorage() {
        extras.update { it.copy(storage = LoadState.Loading) }
        viewModelScope.launch {
            val cache = metadata?.let { repo -> runCatchingApp { repo.cacheStats() } }
            val torrent = torrents?.let { repo -> runCatchingApp { repo.storageUsage() } }
            val image = images?.let { cache -> runCatchingApp { cache.sizeBytes() } }
            val failures = listOfNotNull(cache?.exceptionOrNull(), torrent?.exceptionOrNull(), image?.exceptionOrNull())
            val loaded = StorageInfo(cache?.getOrNull(), torrent?.getOrNull(), image?.getOrNull())
            val nothing = loaded.cache == null && loaded.torrents == null && loaded.imageBytes == null
            val result: LoadState<StorageInfo> = if (failures.isNotEmpty() && nothing) {
                LoadState.Err(failures.first().toAppError())
            } else {
                LoadState.Ok(loaded)
            }
            extras.update { it.copy(storage = result) }
        }
    }

    /**
     * "Limpiar caché" (checklist system.clear-cache): the metadata cache table and the image disk cache (Coil). Both are tried even when
     * one fails; the freed bytes are measured (before - after), the first failure is reported instead.
     */
    fun clearCache() {
        if (metadata == null && images == null) return
        if (extras.value.clearingCache) return
        extras.update { it.copy(clearingCache = true) }
        viewModelScope.launch {
            val before = measureClearable()
            val metaResult = metadata?.let { repo -> runCatchingApp { repo.clearCache() } }
            val imageResult = images?.let { cache -> runCatchingApp { cache.clear() } }
            val after = measureClearable()
            extras.update { it.copy(clearingCache = false) }
            val failure = metaResult?.exceptionOrNull() ?: imageResult?.exceptionOrNull()
            _events.trySend(
                if (failure != null) SettingsEvent.Failed(failure.toAppError()) else SettingsEvent.CacheCleared(freedBytes(before, after)),
            )
            refreshStorage()
        }
    }

    /** Metadata cache + image disk cache bytes right now (an unreadable figure counts as 0). */
    private suspend fun measureClearable(): Long {
        val meta = metadata?.let { repo -> runCatchingApp { repo.cacheStats().bytes }.getOrNull() } ?: 0L
        val img = images?.let { cache -> runCatchingApp { cache.sizeBytes() }.getOrNull() } ?: 0L
        return meta + img
    }

    /** parity wave A (worker q, checklist system.reset-app): wipe everything through [AppReset]; the UI confirmed first. */
    fun resetApp() {
        val r = reset ?: return
        if (extras.value.resetting) return
        extras.update { it.copy(resetting = true) }
        viewModelScope.launch {
            val result = runCatchingApp { r.resetAll().getOrThrow() }
            extras.update { it.copy(resetting = false) }
            _events.trySend(result.fold(onSuccess = { SettingsEvent.ResetDone }, onFailure = { SettingsEvent.Failed(it.toAppError()) }))
        }
    }

    // ---------------------------------------------------------------- helpers

    private fun updatePrefs(transform: (SourcePrefs) -> SourcePrefs) {
        viewModelScope.launch {
            settingsRepo.update { it.copy(sourcePrefs = transform(it.sourcePrefs)) }
            _events.trySend(SettingsEvent.Saved)
        }
    }

    /** parity wave A (15 §3.2): generic setter for rows owned by other modules (`PlayerSettingsExtras` of worker p). Announces "saved". */
    fun updateSettings(transform: (AppSettings) -> AppSettings) = update(announce = true, transform)

    private fun update(announce: Boolean = false, transform: (AppSettings) -> AppSettings) {
        viewModelScope.launch {
            settingsRepo.update(transform)
            if (announce) _events.trySend(SettingsEvent.Saved)
        }
    }

    private data class Extras(
        val syncing: Boolean = false,
        val storage: LoadState<StorageInfo> = LoadState.Loading,
        val clearingCache: Boolean = false,
        val resetting: Boolean = false,
    )
}

/** Pure list move used by the provider priority editor; ignores unknown ids and clamps to the list bounds. */
fun List<String>.moved(id: String, delta: Int): List<String> {
    val from = indexOf(id)
    if (from < 0) return this
    val to = (from + delta).coerceIn(0, lastIndex)
    if (to == from) return this
    return toMutableList().also {
        it.removeAt(from)
        it.add(to, id)
    }
}
