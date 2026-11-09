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
    // --- parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.9); persisted by SettingsCodec (contract) ---
    /** Worker r: web downloads (folder, concurrency, Wi-Fi only, storage limit, auto-download, auto-evict). */
    val downloads: DownloadSettings = DownloadSettings(),
    /** Worker r: local notification when a followed series airs an episode (`AiringRepository`). */
    val airingReminders: Boolean = true,
    /** Worker r: compare installed extensions with their repositories at app start (`ExtensionRepository.checkUpdates`). */
    val extensionUpdateCheck: Boolean = true,
    /** Worker s: manga reader (mode, fit, direction, loading, behaviour, keymap). */
    val reader: ReaderSettings = ReaderSettings(),
    /** Worker s: novel reader typography and flow. */
    val novelReader: NovelReaderSettings = NovelReaderSettings(),
    /** Worker s: narration engine (no API key here: `SecretKeys.ttsApiKey`). */
    val tts: TtsSettings = TtsSettings(),
    /** Worker s: separate title language for manga / novels (checklist manga.title-language). */
    val mangaTitleLanguage: TitleLanguage = TitleLanguage.ROMAJI,
    /** Worker t: poll AniList notifications (15 min + on foreground) and show the badge. */
    val socialNotifications: Boolean = true,
    /**
     * Worker r (checklist settings.privacy.mature-content): how `isAdult` titles are presented once [showAdult] lets them in. Home, Search,
     * Details and the manga home (s) all read it.
     */
    val matureContent: MatureContentMode = MatureContentMode.WARN,
) {
    companion object {
        const val DEFAULT_PALETTE_ID = "tatakai-violet"
        val DEFAULT_PROVIDER_PRIORITY: List<String> = listOf("anilist", "anizip", "kitsu")

        /**
         * parity wave B (16 §3.1, worker r, checklist settings.metadata.extra-providers): every metadata provider the app knows. Ids absent
         * from [providerPriority] are disabled; Settings › Metadatos lets the user enable / order them. `mal` / `shikimori` / `tenrai` are
         * `MetadataProvider`s of :core:metadata (a provider that no longer answers is dropped by r with a note in KNOWN-ISSUES).
         */
        val KNOWN_PROVIDER_IDS: List<String> = listOf("anilist", "anizip", "kitsu", "mal", "shikimori", "tenrai")
    }
}

// --- android wave (12 §3.3) ---

enum class ThemeMode { SYSTEM, LIGHT, DARK }

/** Settings › Apariencia › Idioma: system / en / es only (decision 4 of 06). */
enum class AppLanguage(val tag: String?) { SYSTEM(null), EN("en"), ES("es") }

/** parity wave A (15 §3.5, worker o): preferred title (`Media.titles` keys `romaji` / `english` / `native`, falling back to `Media.title`). */
enum class TitleLanguage(val key: String) { ROMAJI("romaji"), ENGLISH("english"), NATIVE("native") }

/**
 * parity wave B (16 §3.9, worker r, checklist settings.privacy.mature-content): presentation of `isAdult` titles (classic Privacy ›
 * Mature content). SHOW = as any other title; BLUR = blurred poster in rows / results until tapped; WARN = confirmation dialog before
 * opening the title (once per title, remembered privately by the screen). Persisted by SettingsCodec as the enum name.
 */
enum class MatureContentMode(val id: String) { SHOW("show"), BLUR("blur"), WARN("warn") }

/**
 * Sections of the single Settings screen (prototype order) + the Torrents subpage. Deep links: `Route.SettingsSection(section)`.
 * 2026-11-09 amendment: the hosted-backend `ACCOUNT` section is gone; [ACCOUNTS] (AniList / MAL) takes its place in the order.
 */
enum class SettingsSection(val id: String) {
    APPEARANCE("appearance"),
    PLAYER("player"),
    METADATA("metadata"),

    /** Parity wave A (worker q): AniList / MAL accounts (the app's only identity), lists sync, import / export. */
    ACCOUNTS("accounts"),
    LINK("link"),
    TORRENTS("torrents"),

    /** Parity wave B (worker r): web downloads (`DownloadsSettingsSection` of :feature:downloads). */
    DOWNLOADS("downloads"),

    /** Parity wave B (worker s): manga reader + novel reader + narrator (`ReaderSettingsSection` / `NovelReaderSettingsSection`). */
    READER("reader"),
    DATA("data"),
    ABOUT("about"),
}
