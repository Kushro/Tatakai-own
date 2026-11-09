// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.LogLevel
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.LogSink
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
 * New-version notice. Worker q: GitHub Releases API (`/repos/<owner>/<repo>/releases/latest`) of the repository that publishes the
 * Android APK, compared with `BuildConfig.VERSION_NAME`-like input injected through the constructor; at most one request per 24 h
 * (stamp in the settings DataStore), none when `AppSettings.updateCheck` is off.
 */
class UpdateRepositoryImpl : UpdateRepository {
    override val latest: StateFlow<UpdateInfo?> = MutableStateFlow<UpdateInfo?>(null)

    override suspend fun check(force: Boolean): Result<UpdateInfo?> = TODO("worker q")
}
