// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.torrent.TorrentLibraryItem

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r): the rows Home gains in wave B. Kept apart from
// HomeUiState.feeds (the wave android rows) so each new row loads, fails and retries on its own.

/** Top 10 tabs (classic TopAnimeSection: Hoy / Semana / Mes). */
enum class TopWindow { TODAY, WEEK, MONTH }

/** One "infinite" genre row appended when Home is scrolled to the end (classic InfiniteHomeSections). */
data class GenreRow(val genre: String, val state: LoadState<List<Media>>)

/** "Porque viste <title>" (classic BecauseYouWatched). */
data class BecauseRow(val sourceTitle: String, val state: LoadState<List<Media>>)

/** Row "Descargando": torrents of :core:torrent (q) + web downloads of :core:download (r). */
data class DownloadActivity(val torrentActive: Int, val webActive: Int, val web: List<DownloadItem>, val torrents: List<TorrentLibraryItem>) {
    val total: Int get() = torrentActive + webActive
}

data class HomeDiscoverState(
    val latest: LoadState<List<AiringEpisode>> = LoadState.Loading,
    val topWindow: TopWindow = TopWindow.TODAY,
    val top10: LoadState<List<Media>> = LoadState.Loading,
    val upcoming: LoadState<List<Media>> = LoadState.Loading,
    val favourites: LoadState<List<Media>> = LoadState.Loading,
    val because: BecauseRow? = null,
    val genreCloud: List<String> = emptyList(),
    val genreRows: List<GenreRow> = emptyList(),
    /** True once every genre of [genreCloud] has a row. */
    val genreRowsDone: Boolean = false,
    val followed: LoadState<List<AiringEpisode>> = LoadState.Loading,
    val forYou: LoadState<List<RecommendationItem>> = LoadState.Loading,
    val activity: DownloadActivity? = null,
    /** extension id -> display name (custom-provider rows show the NAME, not the id). */
    val extensionNames: Map<String, String> = emptyMap(),
    val matureContent: MatureContentMode = MatureContentMode.WARN,
) {
    /** Any wave B row with something to show (used by [HomeUiState.isEmpty]). */
    val hasContent: Boolean
        get() = listOf(latest, top10, upcoming, favourites, followed).any { (it as? LoadState.Ok<*>)?.value.let { v -> v is List<*> && v.isNotEmpty() } } ||
            (forYou as? LoadState.Ok)?.value?.isNotEmpty() == true ||
            genreRows.any { (it.state as? LoadState.Ok)?.value?.isNotEmpty() == true } ||
            (activity?.total ?: 0) > 0
}

/** Pure helpers of Home (JVM-tested). */
internal object HomeLogic {
    /** Classic mature detection on the fields the Next `Media` carries: AniList puts every `isAdult` title under the genre Hentai. */
    fun isMature(media: Media): Boolean = media.genres.any { it.equals("Hentai", ignoreCase = true) }

    /** "Últimos episodios": aired in the last 7 days, newest first, one card per series (its latest episode). */
    fun latestEpisodes(schedule: List<AiringEpisode>, nowSec: Long, limit: Int = 20): List<AiringEpisode> =
        schedule.filter { it.airingAtEpochSec <= nowSec }
            .sortedByDescending { it.airingAtEpochSec }
            .distinctBy { it.media.ref.anilistId ?: it.media.ref.malId ?: it.media.title.hashCode() }
            .take(limit)

    /**
     * Coordinates of a provider title in `HistoryEntry.animeId` (`"ext:<extension>:<source>:<item>"`, `ProviderEntryRef.historyId`).
     * Extension ids may contain `:` (`server:<id>/<ext>`), so the source and the item are taken from the end.
     */
    fun parseProviderHistoryId(animeId: String): Triple<String, String, String>? {
        if (!animeId.startsWith("ext:")) return null
        val rest = animeId.removePrefix("ext:")
        val item = rest.substringAfterLast(':', "")
        val head = rest.substringBeforeLast(':', "")
        val source = head.substringAfterLast(':', "")
        val ext = head.substringBeforeLast(':', "")
        if (ext.isEmpty() || source.isEmpty() || item.isEmpty()) return null
        return Triple(ext, source, item)
    }

    /** Header label of an extension row: "<row> · <extension name>" when the name is known. */
    fun extensionRowSubtitle(row: ExtensionHomeRow, names: Map<String, String>): String = names[row.extensionId] ?: row.extensionId

    fun activity(torrentActive: Int, web: List<DownloadItem>, torrents: List<TorrentLibraryItem>): DownloadActivity {
        val webActive = web.filter { it.state == DownloadState.DOWNLOADING || it.state == DownloadState.QUEUED }
        val torrentRows = torrents.filter { it.progress < 1.0 && it.state != app.tatakai.next.model.torrent.TorrentState.PAUSED }
        return DownloadActivity(torrentActive, webActive.size, webActive, torrentRows)
    }
}
