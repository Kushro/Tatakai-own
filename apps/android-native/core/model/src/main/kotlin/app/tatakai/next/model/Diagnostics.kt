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
 * "Aviso de versión nueva" (Settings › Acerca de): compares the installed `versionName` with the newest release of the feed
 * configured by worker q (GitHub Releases API of the repository; only that host, nothing is sent about the device). At most one
 * request per 24 h unless [check] is forced; honours `AppSettings.updateCheck`.
 */
interface UpdateRepository {
    /** Last known newer release; null = up to date or never checked. */
    val latest: StateFlow<UpdateInfo?>

    /** Returns the newer release, or null when up to date. */
    suspend fun check(force: Boolean = false): Result<UpdateInfo?>
}
