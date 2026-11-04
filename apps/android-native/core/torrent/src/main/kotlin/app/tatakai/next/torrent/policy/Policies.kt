// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

import app.tatakai.next.model.torrent.TorrentSettings

// Pure decisions of the torrent engine (docs/redesign/10 §4.3, 11 §4.1, §4.3, §7, 12 §0 decision 4). No Android / libtorrent types,
// so they are covered by JVM tests.

/** What happens to a torrent when the player leaves it (after the batch grace period). */
enum class AfterWatchAction {
    /** Not persisted and not a background download: remove it together with its files. */
    REMOVE_WITH_FILES,

    /** Persisted, but it must not keep transferring (background downloads off, or complete without seeding). */
    PAUSE,

    /** Persisted background download that is still incomplete (or a torrent the user asked to keep seeding). */
    KEEP_RUNNING,
}

/**
 * After-watch policy of desktop `torrent_settings::playback_ended` (10 §4.3) + the Android "no seeding after completion" rule:
 * - still streamed by another player (open streams) -> null (nothing to do);
 * - not persisted and not a background download -> [AfterWatchAction.REMOVE_WITH_FILES];
 * - complete -> [AfterWatchAction.KEEP_RUNNING] only with [seed] (and background allowed), else [AfterWatchAction.PAUSE];
 * - incomplete -> [AfterWatchAction.KEEP_RUNNING] when background downloads are enabled, else [AfterWatchAction.PAUSE].
 * [watchedToEnd] is informative (desktop does not use it either).
 */
@Suppress("UNUSED_PARAMETER")
fun afterWatchAction(
    persist: Boolean,
    backgroundDownload: Boolean,
    backgroundEnabled: Boolean,
    complete: Boolean,
    seed: Boolean,
    openStreams: Int,
    watchedToEnd: Boolean = false,
): AfterWatchAction? {
    if (openStreams > 0) return null
    if (!persist && !backgroundDownload) return AfterWatchAction.REMOVE_WITH_FILES
    if (complete) return if (seed && backgroundEnabled) AfterWatchAction.KEEP_RUNNING else AfterWatchAction.PAUSE
    return if (backgroundEnabled) AfterWatchAction.KEEP_RUNNING else AfterWatchAction.PAUSE
}

/** Network / battery facts the run policy needs. */
data class DeviceConditions(
    val connected: Boolean = true,
    val metered: Boolean = false,
    val batteryLow: Boolean = false,
    /** Android 15 `onTimeout` of the dataSync service paused background work until the user resumes. */
    val pausedBySystem: Boolean = false,
)

/** Background downloads may transfer now (11 §4.3 step 5, §7). Playback is never paused by these rules. */
fun backgroundAllowed(settings: TorrentSettings, conditions: DeviceConditions): Boolean =
    conditions.connected &&
        !conditions.pausedBySystem &&
        !(settings.wifiOnly && conditions.metered) &&
        !(settings.pauseOnLowBattery && conditions.batteryLow)

/** Facts about one torrent for [shouldRun]. */
data class RunFacts(
    val streaming: Boolean,
    /** play() was called and playbackEnded (+ grace) has not run yet. */
    val playing: Boolean,
    val userPaused: Boolean,
    /** The user pressed Resume / "Download only this one" in this engine session. */
    val userResumed: Boolean,
    val complete: Boolean,
    val persist: Boolean,
    val backgroundDownload: Boolean,
    val seed: Boolean,
)

/**
 * Whether a torrent should be transferring right now. Streams and active playback always run (the metered confirmation already
 * happened in the player); a user pause wins over everything else; complete torrents only keep running when [RunFacts.seed];
 * persisted / background torrents follow the background settings and the device conditions.
 */
fun shouldRun(facts: RunFacts, settings: TorrentSettings, conditions: DeviceConditions): Boolean {
    if (facts.streaming || facts.playing) return true
    if (facts.userPaused) return false
    val allowed = backgroundAllowed(settings, conditions)
    if (facts.complete) return facts.seed && allowed
    if (facts.userResumed) return conditions.connected && !conditions.pausedBySystem
    val eligible = (facts.persist || facts.backgroundDownload) && settings.backgroundDownloads && settings.backgroundAck
    return eligible && allowed
}

/**
 * Upload cap in bytes/s: on metered networks at most 1 KiB/s unless `uploadOnMobileData` (11 §2.4), else the user limit
 * (0 = unlimited).
 */
fun uploadLimitBytes(settings: TorrentSettings, metered: Boolean): Int {
    val user = settings.uploadLimitKib.coerceIn(0, TorrentSettings.MAX_LIMIT_KIB) * 1024
    if (!metered || settings.uploadOnMobileData) return user
    return if (user == 0) METERED_UPLOAD_BPS else minOf(user, METERED_UPLOAD_BPS)
}

const val METERED_UPLOAD_BPS: Int = 1024

/** qBittorrent-style flag letters `D U I P E X H S` (legend keys `torrent_peers_flag_<letter>`), space separated. */
fun peerFlagLetters(
    downloading: Boolean,
    uploading: Boolean,
    incoming: Boolean,
    utp: Boolean,
    encrypted: Boolean,
    fromPex: Boolean,
    fromDht: Boolean,
    snubbed: Boolean,
): String = buildList {
    if (downloading) add("D")
    if (uploading) add("U")
    if (incoming) add("I")
    if (utp) add("P")
    if (encrypted) add("E")
    if (fromPex) add("X")
    if (fromDht) add("H")
    if (snubbed) add("S")
}.joinToString(" ")
