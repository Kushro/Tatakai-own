package app.tatakai.next.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AccountSuspendedException
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.HistoryRepository
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
import app.tatakai.next.model.UserSession
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
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class SignInError { INVALID, SUSPENDED }

/** Data & storage figures (Settings › Datos y almacenamiento): metadata cache + torrent storage. Either may be unknown. */
data class StorageInfo(val cache: MetadataCacheStats?, val torrents: TorrentStorage?)

data class SettingsUiState(
    val settings: AppSettings = AppSettings(),
    val session: UserSession? = null,
    val signingIn: Boolean = false,
    val signInError: SignInError? = null,
    // --- android wave (12 §4 b.4) ---
    val lastSyncedAt: Long? = null,
    val syncing: Boolean = false,
    val storage: LoadState<StorageInfo> = LoadState.Loading,
    val clearingCache: Boolean = false,
)

sealed interface SettingsEvent {
    /** Prototype `snack(t("settings.saved"))` after a change. */
    data object Saved : SettingsEvent
    data class CacheCleared(val freedBytes: Long) : SettingsEvent
    data class Failed(val cause: AppError) : SettingsEvent
    data object Synced : SettingsEvent
}

/**
 * Settings as one sectioned screen. Every field saves immediately through [SettingsRepository]. [history], [metadata] and
 * [torrents] are optional so the existing tests (and a build without the torrent engine) keep working.
 */
class SettingsViewModel(
    private val settingsRepo: SettingsRepository,
    private val auth: AuthRepository,
    private val history: HistoryRepository? = null,
    private val metadata: MetadataRepository? = null,
    private val torrents: TorrentRepository? = null,
) : ViewModel() {

    private val signInState = MutableStateFlow(SignInProgress())
    private val extras = MutableStateFlow(Extras())
    private val _events = Channel<SettingsEvent>(Channel.BUFFERED)
    val events: Flow<SettingsEvent> = _events.receiveAsFlow()

    val state: StateFlow<SettingsUiState> = combine(
        settingsRepo.settings,
        auth.session,
        signInState,
        history?.lastSyncedAt ?: flowOf(null),
        extras,
    ) { s, session, progress, synced, x ->
        SettingsUiState(
            settings = s,
            session = session,
            signingIn = progress.working,
            signInError = progress.error,
            lastSyncedAt = synced,
            syncing = x.syncing,
            storage = x.storage,
            clearingCache = x.clearingCache,
        )
    }.stateIn(
        viewModelScope,
        SharingStarted.WhileSubscribed(5_000),
        SettingsUiState(settings = settingsRepo.settings.value, session = auth.session.value),
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

    // ---------------------------------------------------------------- account

    fun signIn(email: String, password: String) {
        if (signInState.value.working) return
        signInState.value = SignInProgress(working = true)
        viewModelScope.launch {
            val result = auth.signIn(email.trim(), password)
            signInState.value = result.fold(
                onSuccess = { SignInProgress() },
                onFailure = { e ->
                    SignInProgress(error = if (e is AccountSuspendedException) SignInError.SUSPENDED else SignInError.INVALID)
                },
            )
        }
    }

    fun dismissSignInError() = signInState.update { it.copy(error = null) }

    fun signOut() {
        viewModelScope.launch { auth.signOut() }
    }

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
            val failures = listOfNotNull(cache?.exceptionOrNull(), torrent?.exceptionOrNull())
            val loaded = StorageInfo(cache?.getOrNull(), torrent?.getOrNull())
            val result: LoadState<StorageInfo> = if (failures.isNotEmpty() && loaded.cache == null && loaded.torrents == null) {
                LoadState.Err(failures.first().toAppError())
            } else {
                LoadState.Ok(loaded)
            }
            extras.update { it.copy(storage = result) }
        }
    }

    fun clearCache() {
        val repo = metadata ?: return
        if (extras.value.clearingCache) return
        extras.update { it.copy(clearingCache = true) }
        viewModelScope.launch {
            val before = (extras.value.storage as? LoadState.Ok)?.value?.cache?.bytes ?: 0L
            val result = runCatchingApp { repo.clearCache() }
            extras.update { it.copy(clearingCache = false) }
            _events.trySend(
                result.fold(onSuccess = { SettingsEvent.CacheCleared(before) }, onFailure = { SettingsEvent.Failed(it.toAppError()) }),
            )
            refreshStorage()
        }
    }

    // ---------------------------------------------------------------- helpers

    private fun updatePrefs(transform: (SourcePrefs) -> SourcePrefs) {
        viewModelScope.launch {
            settingsRepo.update { it.copy(sourcePrefs = transform(it.sourcePrefs)) }
            _events.trySend(SettingsEvent.Saved)
        }
    }

    private fun update(announce: Boolean = false, transform: (AppSettings) -> AppSettings) {
        viewModelScope.launch {
            settingsRepo.update(transform)
            if (announce) _events.trySend(SettingsEvent.Saved)
        }
    }

    private data class SignInProgress(val working: Boolean = false, val error: SignInError? = null)

    private data class Extras(
        val syncing: Boolean = false,
        val storage: LoadState<StorageInfo> = LoadState.Loading,
        val clearingCache: Boolean = false,
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
