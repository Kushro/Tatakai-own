package app.tatakai.next.model

import app.tatakai.next.model.torrent.TorrentSettings

enum class ContrastPref { STANDARD, MEDIUM, HIGH }

/** [paletteId] is a key of packages/tokens palettes (default "tatakai-violet"). */
data class AppSettings(
    val paletteId: String = DEFAULT_PALETTE_ID,
    val amoled: Boolean = false,
    val contrast: ContrastPref = ContrastPref.STANDARD,
    val dynamicColor: Boolean = true,
    val providerPriority: List<String> = DEFAULT_PROVIDER_PRIORITY,
    val playerHeadersDebug: Boolean = false,
    // --- android wave (12 §3.3); persisted by worker a (DataStore), defaults = prototype ui.js / 11 §2.4 ---
    val themeMode: ThemeMode = ThemeMode.SYSTEM,
    val reduceMotion: Boolean = false,
    val language: AppLanguage = AppLanguage.SYSTEM,
    val playerEngine: PlayerEngine = PlayerEngine.MEDIA3,
    /** Prototype data.js PRESETS id: "off" | "anime4k-a" | "anime4k-aa" | "anime4k-b" | "anime4k-c" | "fsr". Only the MPV engine applies it. */
    val playerPreset: String = "off",
    /** Source selection; the mode (Auto / Manual) is [SourcePrefs.mode] (there is no separate sourceMode field). */
    val sourcePrefs: SourcePrefs = SourcePrefs(),
    val autoNext: Boolean = true,
    val skipIntro: Boolean = true,
    val subtitleStyle: SubtitleStyle = SubtitleStyle(),
    /** Torrent client settings incl. backgroundAck (desktop keeps them in torrent-settings.json; Android in the same DataStore). */
    val torrent: TorrentSettings = TorrentSettings(),
    /** TTL of cached media details and episode lists (feeds keep their 15 min). 1..168. */
    val metadataCacheTtlHours: Int = 24,
    /** Package of the external player (mpv-android `is.xyz.mpv`, VLC `org.videolan.vlc`); null = system chooser. */
    val externalPlayerPackage: String? = null,
    // --- parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.5); persisted by SettingsCodec (contract), defaults = classic app ---
    /** Worker p: subtitle translation (provider, mode, target language, credentials). */
    val subtitleTranslation: SubtitleTranslation = SubtitleTranslation(),
    /** Worker p: second subtitle track shown under the first (ISO 639-1); null = off. */
    val secondarySubLang: String? = null,
    /** Worker p: seconds of the "next episode" countdown when [autoNext] is on. 3..30 (checklist player.auto-next.countdown). */
    val autoNextCountdownSecs: Int = 10,
    /** Worker p: loudness normalisation (classic "stable volume"). */
    val stableVolume: Boolean = false,
    /** Worker p: keep playing audio when the app goes to the background (`PlaybackService`, MediaSession notification). */
    val backgroundPlayback: Boolean = true,
    /**
     * Worker p: hardware-keyboard / remote shortcuts, `action id -> key name` (`"seek_forward" -> "L"`). Empty = defaults.
     * Action ids and the default map live in :core:player (`PlayerKeymap`).
     */
    val playerKeymap: Map<String, String> = emptyMap(),
    /** Worker q: advance AniList / MAL progress when an episode completes (classic Integrations auto-sync). */
    val autoListProgress: Boolean = true,
    /** Worker q: playing a title that is not on the list adds it as WATCHING (checklist lists.auto-add). Off by default. */
    val autoListAdd: Boolean = false,
    /** Worker q: check the release feed once a day and show the new-version notice in Acerca de. */
    val updateCheck: Boolean = true,
    /** Worker o: include 18+ titles in search and feeds (classic "mature content" toggle). */
    val showAdult: Boolean = false,
    /** Worker o: which title to show in cards, rows and Details (checklist details.title-language). */
    val titleLanguage: TitleLanguage = TitleLanguage.ROMAJI,
    // --- 2026-11-09 amendment (plan 14 §0.1 #9 / #11), persisted by SettingsCodec (contract) ---
    /** Worker q: sync the watch history (exact resume position) with the paired PC over Tatakai Link. Local-only when off. */
    val historySyncOverLink: Boolean = true,
    /** Worker q: the first-run wizard was completed (or skipped). The shell shows `OnboardingScreen` while false. */
    val onboardingDone: Boolean = false,
) {
    companion object {
        const val DEFAULT_PALETTE_ID = "tatakai-violet"
        val DEFAULT_PROVIDER_PRIORITY: List<String> = listOf("anilist", "anizip", "kitsu")
    }
}

// --- android wave (12 §3.3) ---

enum class ThemeMode { SYSTEM, LIGHT, DARK }

/** Settings › Apariencia › Idioma: system / en / es only (decision 4 of 06). */
enum class AppLanguage(val tag: String?) { SYSTEM(null), EN("en"), ES("es") }

/** parity wave A (15 §3.5, worker o): preferred title (`Media.titles` keys `romaji` / `english` / `native`, falling back to `Media.title`). */
enum class TitleLanguage(val key: String) { ROMAJI("romaji"), ENGLISH("english"), NATIVE("native") }

/**
 * Sections of the single Settings screen (prototype order) + the Torrents subpage. Deep links: `Route.SettingsSection(section)`.
 * 2026-11-09 amendment: the Supabase `ACCOUNT` section is gone; [ACCOUNTS] (AniList / MAL) takes its place in the order.
 */
enum class SettingsSection(val id: String) {
    APPEARANCE("appearance"),
    PLAYER("player"),
    METADATA("metadata"),

    /** Parity wave A (worker q): AniList / MAL accounts (the app's only identity), lists sync, import / export. */
    ACCOUNTS("accounts"),
    LINK("link"),
    TORRENTS("torrents"),
    DATA("data"),
    ABOUT("about"),
}
