package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.MutablePreferences
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.intPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.model.SubtitleTranslation
import app.tatakai.next.model.torrent.TorrentSettings
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

/**
 * DataStore-backed settings. The first value is read synchronously at construction (a few ms on a cold start) so the
 * theme never flashes the defaults before the stored palette arrives.
 *
 * android wave (12 §3.3, §4 a): every new [AppSettings] field has its own key; objects ([SourcePrefs], [SubtitleStyle],
 * [TorrentSettings] incl. its sealed `StorageLocation`) are stored as kotlinx.serialization JSON. A missing or unreadable value reads
 * as its default, so the existing keys of older installs keep working and nothing is lost.
 */
class SettingsRepositoryImpl(
    private val store: DataStore<Preferences>,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) : SettingsRepository {

    override val settings: StateFlow<AppSettings> = run {
        val initial = runBlocking(Dispatchers.IO) { SettingsCodec.read(store.data.first()) }
        store.data.map { SettingsCodec.read(it) }.stateIn(scope, SharingStarted.Eagerly, initial)
    }

    override suspend fun update(transform: (AppSettings) -> AppSettings) {
        store.edit { prefs -> SettingsCodec.write(prefs, transform(SettingsCodec.read(prefs))) }
    }
}

/** Mapping between [AppSettings] and DataStore keys (pure: unit-tested on the JVM with an in-memory store). */
internal object SettingsCodec {
    const val TTL_MIN_HOURS = 1
    const val TTL_MAX_HOURS = 168

    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        coerceInputValues = true
        explicitNulls = false
    }

    private val PALETTE = stringPreferencesKey("palette_id")
    private val AMOLED = booleanPreferencesKey("amoled")
    private val CONTRAST = stringPreferencesKey("contrast")
    private val DYNAMIC = booleanPreferencesKey("dynamic_color")
    private val PRIORITY = stringPreferencesKey("provider_priority")
    private val HEADERS_DEBUG = booleanPreferencesKey("player_headers_debug")

    // --- android wave ---
    private val THEME_MODE = stringPreferencesKey("theme_mode")
    private val REDUCE_MOTION = booleanPreferencesKey("reduce_motion")
    private val LANGUAGE = stringPreferencesKey("language")
    private val PLAYER_ENGINE = stringPreferencesKey("player_engine")
    private val PLAYER_PRESET = stringPreferencesKey("player_preset")
    private val SOURCE_PREFS = stringPreferencesKey("source_prefs_json")
    private val AUTO_NEXT = booleanPreferencesKey("auto_next")
    private val SKIP_INTRO = booleanPreferencesKey("skip_intro")
    private val SUBTITLE_STYLE = stringPreferencesKey("subtitle_style_json")
    private val TORRENT = stringPreferencesKey("torrent_settings_json")
    private val METADATA_TTL = intPreferencesKey("metadata_cache_ttl_hours")
    private val EXTERNAL_PLAYER = stringPreferencesKey("external_player_package")

    // --- parity wave A (15 §3.5): one key per new AppSettings field; objects as JSON (contract, so p / o / q never edit this file) ---
    private val SUBTITLE_TRANSLATION = stringPreferencesKey("subtitle_translation_json")
    private val SECONDARY_SUB_LANG = stringPreferencesKey("secondary_sub_lang")
    private val AUTO_NEXT_COUNTDOWN = intPreferencesKey("auto_next_countdown_secs")
    private val STABLE_VOLUME = booleanPreferencesKey("stable_volume")
    private val BACKGROUND_PLAYBACK = booleanPreferencesKey("background_playback")
    private val PLAYER_KEYMAP = stringPreferencesKey("player_keymap_json")
    private val AUTO_LIST_PROGRESS = booleanPreferencesKey("auto_list_progress")
    private val UPDATE_CHECK = booleanPreferencesKey("update_check")
    private val SHOW_ADULT = booleanPreferencesKey("show_adult")
    private val AUTO_LIST_ADD = booleanPreferencesKey("auto_list_add")
    private val TITLE_LANGUAGE = stringPreferencesKey("title_language")
    private val KEYMAP_SERIALIZER = MapSerializer(String.serializer(), String.serializer())
    const val COUNTDOWN_MIN_SECS = 3
    const val COUNTDOWN_MAX_SECS = 30

    private val PRESETS = setOf("off", "anime4k-a", "anime4k-aa", "anime4k-b", "anime4k-c", "fsr")

    fun read(p: Preferences): AppSettings {
        val d = AppSettings()
        return AppSettings(
            paletteId = p[PALETTE] ?: d.paletteId,
            amoled = p[AMOLED] ?: d.amoled,
            contrast = enumOr(p[CONTRAST], d.contrast),
            dynamicColor = p[DYNAMIC] ?: d.dynamicColor,
            providerPriority = p[PRIORITY]?.split(',')?.filter { it.isNotBlank() } ?: d.providerPriority,
            playerHeadersDebug = p[HEADERS_DEBUG] ?: d.playerHeadersDebug,
            themeMode = enumOr(p[THEME_MODE], d.themeMode),
            reduceMotion = p[REDUCE_MOTION] ?: d.reduceMotion,
            language = enumOr(p[LANGUAGE], d.language),
            playerEngine = enumOr(p[PLAYER_ENGINE], d.playerEngine),
            playerPreset = p[PLAYER_PRESET]?.takeIf { it in PRESETS } ?: d.playerPreset,
            sourcePrefs = decode(p[SOURCE_PREFS], SourcePrefs.serializer()) ?: d.sourcePrefs,
            autoNext = p[AUTO_NEXT] ?: d.autoNext,
            skipIntro = p[SKIP_INTRO] ?: d.skipIntro,
            subtitleStyle = decode(p[SUBTITLE_STYLE], SubtitleStyle.serializer())?.sanitizedStyle() ?: d.subtitleStyle,
            torrent = decode(p[TORRENT], TorrentSettings.serializer())?.sanitized() ?: d.torrent,
            metadataCacheTtlHours = (p[METADATA_TTL] ?: d.metadataCacheTtlHours).coerceIn(TTL_MIN_HOURS, TTL_MAX_HOURS),
            externalPlayerPackage = p[EXTERNAL_PLAYER]?.takeIf { it.isNotBlank() } ?: d.externalPlayerPackage,
            subtitleTranslation = decode(p[SUBTITLE_TRANSLATION], SubtitleTranslation.serializer()) ?: d.subtitleTranslation,
            secondarySubLang = p[SECONDARY_SUB_LANG]?.takeIf { it.isNotBlank() } ?: d.secondarySubLang,
            autoNextCountdownSecs = (p[AUTO_NEXT_COUNTDOWN] ?: d.autoNextCountdownSecs).coerceIn(COUNTDOWN_MIN_SECS, COUNTDOWN_MAX_SECS),
            stableVolume = p[STABLE_VOLUME] ?: d.stableVolume,
            backgroundPlayback = p[BACKGROUND_PLAYBACK] ?: d.backgroundPlayback,
            playerKeymap = decode(p[PLAYER_KEYMAP], KEYMAP_SERIALIZER) ?: d.playerKeymap,
            autoListProgress = p[AUTO_LIST_PROGRESS] ?: d.autoListProgress,
            updateCheck = p[UPDATE_CHECK] ?: d.updateCheck,
            showAdult = p[SHOW_ADULT] ?: d.showAdult,
            autoListAdd = p[AUTO_LIST_ADD] ?: d.autoListAdd,
            titleLanguage = enumOr(p[TITLE_LANGUAGE], d.titleLanguage),
        )
    }

    fun write(p: MutablePreferences, s: AppSettings) {
        p[PALETTE] = s.paletteId
        p[AMOLED] = s.amoled
        p[CONTRAST] = s.contrast.name
        p[DYNAMIC] = s.dynamicColor
        p[PRIORITY] = s.providerPriority.distinct().joinToString(",")
        p[HEADERS_DEBUG] = s.playerHeadersDebug
        p[THEME_MODE] = s.themeMode.name
        p[REDUCE_MOTION] = s.reduceMotion
        p[LANGUAGE] = s.language.name
        p[PLAYER_ENGINE] = s.playerEngine.name
        p[PLAYER_PRESET] = s.playerPreset.takeIf { it in PRESETS } ?: "off"
        p[SOURCE_PREFS] = json.encodeToString(SourcePrefs.serializer(), s.sourcePrefs)
        p[AUTO_NEXT] = s.autoNext
        p[SKIP_INTRO] = s.skipIntro
        p[SUBTITLE_STYLE] = json.encodeToString(SubtitleStyle.serializer(), s.subtitleStyle.sanitizedStyle())
        p[TORRENT] = json.encodeToString(TorrentSettings.serializer(), s.torrent.sanitized())
        p[METADATA_TTL] = s.metadataCacheTtlHours.coerceIn(TTL_MIN_HOURS, TTL_MAX_HOURS)
        val pkg = s.externalPlayerPackage?.trim()
        if (pkg.isNullOrEmpty()) p.remove(EXTERNAL_PLAYER) else p[EXTERNAL_PLAYER] = pkg
        // parity wave A
        p[SUBTITLE_TRANSLATION] = json.encodeToString(SubtitleTranslation.serializer(), s.subtitleTranslation)
        val secondary = s.secondarySubLang?.trim()
        if (secondary.isNullOrEmpty()) p.remove(SECONDARY_SUB_LANG) else p[SECONDARY_SUB_LANG] = secondary
        p[AUTO_NEXT_COUNTDOWN] = s.autoNextCountdownSecs.coerceIn(COUNTDOWN_MIN_SECS, COUNTDOWN_MAX_SECS)
        p[STABLE_VOLUME] = s.stableVolume
        p[BACKGROUND_PLAYBACK] = s.backgroundPlayback
        p[PLAYER_KEYMAP] = json.encodeToString(KEYMAP_SERIALIZER, s.playerKeymap)
        p[AUTO_LIST_PROGRESS] = s.autoListProgress
        p[UPDATE_CHECK] = s.updateCheck
        p[SHOW_ADULT] = s.showAdult
        p[AUTO_LIST_ADD] = s.autoListAdd
        p[TITLE_LANGUAGE] = s.titleLanguage.name
    }

    private inline fun <reified E : Enum<E>> enumOr(name: String?, default: E): E =
        name?.let { n -> enumValues<E>().firstOrNull { it.name == n } } ?: default

    private fun <T> decode(raw: String?, serializer: KSerializer<T>): T? =
        raw?.let { runCatching { json.decodeFromString(serializer, it) }.getOrNull() }

    /** Ranges of 12 §3.2: scale 0.5..2.0, opacity 0..1, known font ids. */
    private fun SubtitleStyle.sanitizedStyle(): SubtitleStyle = copy(
        scale = if (scale.isFinite()) scale.coerceIn(0.5f, 2f) else 1f,
        backgroundOpacity = if (backgroundOpacity.isFinite()) backgroundOpacity.coerceIn(0f, 1f) else 0.6f,
        font = font.takeIf { it in FONTS } ?: "outfit",
    )

    private val FONTS = setOf("outfit", "space_grotesk", "system")
}
