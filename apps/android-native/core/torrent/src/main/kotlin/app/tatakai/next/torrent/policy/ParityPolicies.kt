// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

import app.tatakai.next.model.torrent.EpisodeAlignment
import app.tatakai.next.model.torrent.TorrentSettings

// Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, worker q). Pure rules of the new torrent features, unit
// tested: bandwidth schedule (torrent.bandwidth-schedule), cache limit (torrent.cache-limit), episode alignment
// (torrent.episode-alignment) and next-episode preload (torrent.preload-next).

/** The limits in force at [minuteOfDay] (local time, 0..1439): the schedule's inside its window, the base ones outside. */
fun applySchedule(settings: TorrentSettings, minuteOfDay: Int): TorrentSettings {
    val schedule = settings.schedule ?: return settings
    if (!schedule.contains(minuteOfDay)) return settings
    return settings.copy(downloadLimitKib = schedule.downloadLimitKib, uploadLimitKib = schedule.uploadLimitKib)
}

/** A streamed (non-persisted) torrent that counts against the cache limit. */
data class CacheItem(val id: String, val addedUnix: Long, val bytes: Long, val inUse: Boolean)

/**
 * Ids to remove, oldest first, until the cache is at most [limitBytes]; torrents in use are never removed and still count.
 * [limitBytes] <= 0 = unlimited.
 */
fun cacheEvictions(items: List<CacheItem>, limitBytes: Long): List<String> {
    if (limitBytes <= 0) return emptyList()
    var total = items.sumOf { it.bytes.coerceAtLeast(0) }
    if (total <= limitBytes) return emptyList()
    val out = ArrayList<String>()
    for (item in items.filter { !it.inUse }.sortedBy { it.addedUnix }) {
        if (total <= limitBytes) break
        out += item.id
        total -= item.bytes.coerceAtLeast(0)
    }
    return out
}

/** One video file of a torrent for alignment: [episode] = number parsed from the name (null when none). */
data class AlignFile(val index: Int, val path: String, val episode: Int?)

/**
 * Episode of every video file under [alignment]: parsed number + `offset` when the name has one, otherwise `firstEpisode + position`
 * among the videos sorted by path. Files that end up below 1 are left out.
 */
fun alignedEpisodes(files: List<AlignFile>, alignment: EpisodeAlignment): Map<Int, Int> {
    val sorted = files.sortedBy { it.path.lowercase() }
    val out = LinkedHashMap<Int, Int>()
    sorted.forEachIndexed { position, f ->
        val ep = f.episode?.let { it + alignment.offset } ?: (alignment.firstEpisode + position)
        if (ep >= 1) out[f.index] = ep
    }
    return out
}

/** File index for [episode] under [alignment]; null when no file maps to it. */
fun alignedIndex(files: List<AlignFile>, alignment: EpisodeAlignment, episode: Int): Int? =
    alignedEpisodes(files, alignment).entries.firstOrNull { it.value == episode }?.key

/** Fraction of the streamed file after which the next one starts downloading (classic TorrentPreloadController: 80 %). */
const val PRELOAD_AT: Double = 0.8

/**
 * The file to preload after [current]: the video whose episode is the current one + 1, else the next video by path. Null when
 * [current] is the last one or not a video of the list.
 */
fun preloadTarget(files: List<AlignFile>, current: Int): Int? {
    val sorted = files.sortedBy { it.path.lowercase() }
    val pos = sorted.indexOfFirst { it.index == current }
    if (pos < 0) return null
    val ep = sorted[pos].episode
    if (ep != null) sorted.firstOrNull { it.episode == ep + 1 }?.let { return it.index }
    return sorted.getOrNull(pos + 1)?.index
}
