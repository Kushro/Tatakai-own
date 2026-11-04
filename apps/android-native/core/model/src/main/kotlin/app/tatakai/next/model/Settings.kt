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
) {
    companion object {
        const val DEFAULT_PALETTE_ID = "tatakai-violet"
        val DEFAULT_PROVIDER_PRIORITY: List<String> = listOf("anilist", "anizip", "kitsu")
    }
}

/** Only non-secret session info is exposed; tokens stay inside :core:supabase. */
data class UserSession(val userId: String, val email: String?)

// --- android wave (12 §3.3) ---

enum class ThemeMode { SYSTEM, LIGHT, DARK }

/** Settings › Apariencia › Idioma: system / en / es only (decision 4 of 06). */
enum class AppLanguage(val tag: String?) { SYSTEM(null), EN("en"), ES("es") }

/** Sections of the single Settings screen (prototype order) + the Torrents subpage. Deep links: `Route.SettingsSection(section)`. */
enum class SettingsSection(val id: String) {
    APPEARANCE("appearance"),
    PLAYER("player"),
    METADATA("metadata"),
    ACCOUNT("account"),
    LINK("link"),
    TORRENTS("torrents"),
    DATA("data"),
    ABOUT("about"),
}
