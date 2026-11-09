// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.AppReset
import app.tatakai.next.model.LogLevel
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.LogSink
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.UpdateInfo
import app.tatakai.next.model.UpdateRepository
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.io.File

// Parity wave A contract stubs (docs/redesign/15-parity-wave-a-contract-android.md §3.4). Owner: worker q.

/**
 * File-backed app log under [dir] (`filesDir/logs/app.log`, rotated at ~512 KiB into `app.1.log`), installed as `AppLog.sink` by
 * `dataModule`. Settings › Acerca de › Abrir registros shows [recentLines] and shares [logFile] through a `FileProvider` (q declares it in
 * the :core:data manifest with `${applicationId}.logs` authority). Worker q writes the bodies; [log] stays harmless until then.
 */
class AppLogStore(private val dir: File) : LogRepository, LogSink {
    override fun log(level: LogLevel, tag: String, message: String, throwable: Throwable?) {
        // worker q: timestamped line "2026-11-09T21:04:11.000Z I/player: message" (+ throwable class and message), appended off the main thread.
    }

    override suspend fun recentLines(max: Int): List<String> = TODO("worker q")

    override fun logFile(): File? = dir.resolve("app.log").takeIf { it.isFile }

    override suspend fun clear(): Unit = TODO("worker q")
}

/**
 * New-version notice. Worker q: GitHub Releases API (`https://api.github.com/repos/${UpdateRepositoryImpl.REPO}/releases/latest`) of
 * the repository that publishes the Android APK (user decision 2026-11-09: `Kushro/Tatakai-own`), compared with the installed
 * `versionName` injected through the constructor; at most one request per 24 h (stamp in the settings DataStore), none when
 * `AppSettings.updateCheck` is off. Until a release exists the check answers "up to date" (null).
 */
class UpdateRepositoryImpl : UpdateRepository {
    override val latest: StateFlow<UpdateInfo?> = MutableStateFlow<UpdateInfo?>(null)

    override suspend fun check(force: Boolean): Result<UpdateInfo?> = TODO("worker q")

    companion object {
        const val REPO = "Kushro/Tatakai-own"
    }
}

/**
 * [SecretStore] sealed with an AndroidKeyStore AES-GCM key, one JSON file `secrets.bin` under [dir] (`noBackupFilesDir`), the
 * `LinkKeyVault` / `ExtensionServerStore` pattern (key alias `tatakai-secrets`, 12-byte random IV per write, atomic rename). Holds the
 * AniList / MAL token sets, debrid API keys and translation API keys (`SecretKeys`). Worker q writes the bodies; until then every
 * read answers null and writes are dropped, so nothing crashes and nothing is stored in plain text.
 */
class KeystoreSecretStore(private val dir: File) : SecretStore {
    override suspend fun get(key: String): String? = null

    override suspend fun put(key: String, value: String?) {
        // worker q
    }

    override suspend fun clear() {
        // worker q
    }
}

/**
 * "Restablecer la app" (checklist system.reset-app). Worker q: clear the settings DataStore, `TatakaiDatabase.clearAllTables()`, the
 * metadata / image caches, extension state (`ExtensionRepository.remove` for each), the Link pairing (`LinkRepository.forget()`),
 * `SecretStore.clear()`, the app log and `filesDir/shaders`; then the shell recreates the activity and the first-run wizard shows again.
 * The constructor grows with the collaborators q needs (Koin `get()`s in `dataModule`).
 */
class AppResetImpl : AppReset {
    override suspend fun resetAll(): Result<Unit> = TODO("worker q")
}
