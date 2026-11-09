// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.StateFlow
import java.io.File

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.4): app log ("Abrir registros") and the new-version
// notice (desktop worker f, Android worker q). Signatures FINAL; bodies: worker q (:core:data, `dataModule`).

enum class LogLevel { INFO, WARN, ERROR }

/** Where [AppLog] lines go. The default sink drops them; `dataModule` installs the file-backed one. */
fun interface LogSink {
    fun log(level: LogLevel, tag: String, message: String, throwable: Throwable?)
}

/**
 * Process-wide logger every module may call (`AppLog.i("player", "…")`). Lines reach the file that Settings › Acerca de › Abrir
 * registros shows and shares. Never log tokens, URLs with query strings or personal data: the file is meant to be shared in bug reports.
 */
object AppLog {
    @Volatile
    var sink: LogSink = LogSink { _, _, _, _ -> }

    fun i(tag: String, message: String) = sink.log(LogLevel.INFO, tag, message, null)

    fun w(tag: String, message: String, throwable: Throwable? = null) = sink.log(LogLevel.WARN, tag, message, throwable)

    fun e(tag: String, message: String, throwable: Throwable? = null) = sink.log(LogLevel.ERROR, tag, message, throwable)
}

/** The app log file (rotating, app-private). Implemented by worker q; also the [LogSink] installed into [AppLog]. */
interface LogRepository {
    /** Newest last. */
    suspend fun recentLines(max: Int = 500): List<String>

    /** The current log file for sharing (`FileProvider`); null when nothing was written yet. */
    fun logFile(): File?

    suspend fun clear()
}

/** A release newer than the installed one. */
data class UpdateInfo(val version: String, val url: String, val notes: String?, val publishedAtMs: Long?)

/**
 * "Aviso de versión nueva" (Settings › Acerca de): compares the installed `versionName` with the newest GitHub Release of
 * `Kushro/Tatakai-own` (`api.github.com/repos/Kushro/Tatakai-own/releases/latest`; only that host, nothing is sent about the device).
 * At most one request per 24 h unless [check] is forced; honours `AppSettings.updateCheck`. Only a notice: no in-app install.
 */
interface UpdateRepository {
    /** Last known newer release; null = up to date or never checked. */
    val latest: StateFlow<UpdateInfo?>

    /** Returns the newer release, or null when up to date. */
    suspend fun check(force: Boolean = false): Result<UpdateInfo?>
}

// --- 2026-11-09 amendment (plan 14 §0.1): secrets out of plain settings; "restablecer la app" ------------------------------------------

/**
 * Keys of [SecretStore]. The list-provider tokens are the only identity of the app (no own login); the debrid and translation keys
 * used to be settings fields and now live here. Everything is per-install and never leaves the device except to its own service.
 */
object SecretKeys {
    /** JSON of the OAuth token set (access, refresh, expiry) of a list provider: `ListProvider.id`. Owner: worker q. */
    fun listToken(providerId: String): String = "list.$providerId.token"

    /** API key of a debrid service: `DebridService.id`. Owner: worker q. */
    fun debridApiKey(serviceId: String): String = "debrid.$serviceId.api_key"

    /** API key of a subtitle translation provider: `TranslationProvider.id`. Owner: worker p (reads / writes), store: worker q. */
    fun translationApiKey(providerId: String): String = "translate.$providerId.api_key"

    /** parity wave B (16 §3.6, worker s): API key of a narration engine: `TtsEngine.id` (OpenAI, ElevenLabs, OpenAI-compatible). */
    fun ttsApiKey(engineId: String): String = "tts.$engineId.api_key"

    /** parity wave B (16 §3.4, worker r): a SECRET preference of an extension (`ExtensionPrefType.SECRET`). */
    fun extensionPreference(extensionId: String, key: String): String = "ext.$extensionId.pref.$key"
}

/**
 * parity wave B (16 §3.8): a module-private store that "Restablecer la app" must wipe (downloads index + files, followed shows, reader
 * progress, chapter downloads, Stremio addons, social caches). Every new core module binds one (`single { … } bind AppDataWiper::class`);
 * `AppResetImpl` runs `getAll<AppDataWiper>()` after the shared stores. Never throws for a missing file.
 */
fun interface AppDataWiper {
    suspend fun wipe()
}

/**
 * Small key / value store for secrets, sealed with an AndroidKeyStore AES-GCM key under `noBackupFilesDir` (the `LinkKeyVault` /
 * `ExtensionServerStore` pattern). Desktop counterpart: `tk_core::Secrets` (OS keyring, 0600 JSON fallback). Implemented by worker q in
 * :core:data (`KeystoreSecretStore`, bound in `dataModule`); workers p and q read / write through it, never through `AppSettings`.
 */
interface SecretStore {
    suspend fun get(key: String): String?

    /** null removes the entry. */
    suspend fun put(key: String, value: String?)

    /** Removes every secret (sign-out of everything; part of [AppReset.resetAll]). */
    suspend fun clear()
}

/**
 * Settings › Datos › "Restablecer la app" (classic reset-app, checklist `system.reset-app`): wipes settings (DataStore), Room, caches,
 * extension state, the Link pairing and every [SecretStore] entry, then the shell restarts the activity and shows the first-run wizard
 * again (`AppSettings.onboardingDone` is back to false). Implemented by worker q in :core:data (`AppResetImpl`, bound in `dataModule`).
 */
interface AppReset {
    suspend fun resetAll(): Result<Unit>
}
