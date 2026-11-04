// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import app.tatakai.next.model.Container

// android wave (docs/redesign/12 §0.3, §4 d.1): the EXTERNAL engine. ACTION_VIEW with `setDataAndType` to mpv-android / VLC (or the
// system chooser when no package is set), HTTP headers as extras, title and start position. The app shows ExternalPlayerScreen meanwhile
// and records the progress the player returns (mpv-android / VLC answer with a position extra when started for result).

/** What to hand to the external app. [extras] values are String, Long or Array<String>. Pure, unit-tested. */
data class ExternalLaunch(
    val uri: String,
    val mimeType: String,
    val packageName: String?,
    val extras: Map<String, Any>,
)

object ExternalPlayer {
    const val PACKAGE_MPV = "is.xyz.mpv"
    const val PACKAGE_VLC = "org.videolan.vlc"

    /** Known players offered in Settings (worker b) and labelled on the informative screen. */
    val KNOWN_PACKAGES: List<String> = listOf(PACKAGE_MPV, PACKAGE_VLC)

    fun mimeFor(container: Container, url: String): String = when (container) {
        Container.HLS -> "application/x-mpegURL"
        Container.DASH -> "application/dash+xml"
        Container.MP4 -> "video/mp4"
        Container.PROGRESSIVE -> when (url.substringBefore('?').substringAfterLast('.', "").lowercase()) {
            "mkv" -> "video/x-matroska"
            "webm" -> "video/webm"
            "mp4", "m4v" -> "video/mp4"
            else -> "video/*"
        }
    }

    /**
     * Builds the launch for [source]. Headers go as `headers` (String[] of alternating name / value, the MX-style extra mpv-android reads)
     * and as `http-headers` (one `Name: value` per line, VLC); `title`; the start as `position` (ms, Int, mpv-android) and
     * `extra_position` (ms, Long, VLC); `return_result` so the app gets the final position back.
     */
    fun launch(source: PlayableSource, title: String, startMs: Long, packageName: String?): ExternalLaunch {
        val extras = LinkedHashMap<String, Any>()
        extras["title"] = title
        extras["return_result"] = true
        if (startMs > 0) {
            extras["position"] = startMs.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
            extras["extra_position"] = startMs
            extras["from_start"] = false
        }
        if (source.headers.isNotEmpty()) {
            extras["headers"] = source.headers.flatMap { (k, v) -> listOf(k, v) }.toTypedArray()
            extras["http-headers"] = source.headers.entries.joinToString("\r\n") { (k, v) -> "$k: $v" }
        }
        val subs = source.subtitles.map { it.url }
        if (subs.isNotEmpty()) {
            extras["subs"] = subs.toTypedArray()
            extras["subtitles_location"] = subs.first()
        }
        return ExternalLaunch(source.url, mimeFor(source.container, source.url), packageName?.takeIf { it.isNotBlank() }, extras)
    }

    /** The Intent for [launch]; with no package the system chooser is used (`Intent.createChooser`). */
    fun intent(launch: ExternalLaunch, chooserTitle: CharSequence?): Intent {
        val view = Intent(Intent.ACTION_VIEW).apply {
            setDataAndType(Uri.parse(launch.uri), launch.mimeType)
            addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            launch.packageName?.let { setPackage(it) }
            for ((k, v) in launch.extras) {
                when (v) {
                    is String -> putExtra(k, v)
                    is Int -> putExtra(k, v)
                    is Long -> putExtra(k, v)
                    is Boolean -> putExtra(k, v)
                    is Array<*> -> putExtra(k, v.map { it.toString() }.toTypedArray())
                }
            }
        }
        return if (launch.packageName == null) Intent.createChooser(view, chooserTitle) else view
    }

    /** Position (ms) the external player returned: mpv-android `position` (Int), VLC `extra_position` (Long); null when absent. */
    fun resultPositionMs(data: Intent?): Long? {
        val extras = data?.extras ?: return null
        val raw = extras.get("position") ?: extras.get("extra_position") ?: return null
        return (raw as? Number)?.toLong()?.takeIf { it >= 0 }
    }

    /** Duration (ms) the external player returned, when it does. */
    fun resultDurationMs(data: Intent?): Long? {
        val extras = data?.extras ?: return null
        val raw = extras.get("duration") ?: extras.get("extra_duration") ?: return null
        return (raw as? Number)?.toLong()?.takeIf { it > 0 }
    }

    /** User-facing name of [packageName] ("mpv", "VLC"), or null when not installed / not visible. */
    fun appLabel(context: Context, packageName: String?): String? {
        val pkg = packageName?.takeIf { it.isNotBlank() } ?: return null
        return try {
            val pm = context.packageManager
            pm.getApplicationLabel(pm.getApplicationInfo(pkg, 0)).toString()
        } catch (e: PackageManager.NameNotFoundException) {
            null
        }
    }
}
