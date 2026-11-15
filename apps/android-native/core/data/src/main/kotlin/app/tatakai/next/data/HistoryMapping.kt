package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.database.HistoryRow
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.MediaRef
import app.tatakai.next.link.LinkHistoryRow
import kotlinx.coroutines.flow.first
import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter

private val ISO_MILLIS: DateTimeFormatter =
    DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC)

/** `2026-10-27T21:04:11.000Z`, the same shape as JS `new Date().toISOString()` in the legacy app. */
internal fun Instant.toIsoMillis(): String = ISO_MILLIS.format(this)

/** Accepts `Z` and `+00:00` offsets (PostgREST returns the latter, with microseconds). */
internal fun parseInstant(s: String): Instant = OffsetDateTime.parse(s).toInstant()

internal fun HistoryEntry.toRow(): HistoryRow = HistoryRow(
    episodeId = episodeId,
    animeId = animeId,
    title = title,
    poster = poster,
    episode = episode,
    progressSec = progressSec,
    durationSec = durationSec?.takeIf { it > 0 },
    completed = HistoryEntry.isCompleted(progressSec, durationSec, completed),
    watchedAtMs = watchedAt.toEpochMilli(),
    anilistId = anilistId,
    malId = malId,
    syncedAtMs = null,
)

internal fun HistoryRow.toEntry(): HistoryEntry = HistoryEntry(
    animeId = animeId,
    title = title,
    poster = poster,
    episodeId = episodeId,
    episode = episode,
    progressSec = progressSec,
    durationSec = durationSec,
    completed = completed,
    watchedAt = Instant.ofEpochMilli(watchedAtMs),
    anilistId = anilistId,
    malId = malId,
)

/** Row -> Tatakai Link `HistoryEntry` (camelCase JSON of tk-core; desktop contract §3.8). */
internal fun HistoryRow.toRemote(): LinkHistoryRow = LinkHistoryRow(
    animeId = animeId,
    title = title.ifBlank { "Unknown Anime" },
    poster = poster?.takeIf { it.isNotEmpty() },
    episodeId = episodeId,
    episode = episode,
    progressSec = progressSec,
    durationSec = durationSec,
    completed = completed,
    watchedAt = Instant.ofEpochMilli(watchedAtMs).toIsoMillis(),
    anilistId = anilistId,
    malId = malId,
)

internal fun LinkHistoryRow.toRow(): HistoryRow = HistoryRow(
    episodeId = episodeId,
    animeId = animeId,
    title = title,
    poster = poster?.takeIf { it.isNotEmpty() },
    episode = episode,
    progressSec = progressSec,
    durationSec = durationSec?.takeIf { it > 0 },
    completed = completed,
    watchedAtMs = parseInstant(watchedAt).toEpochMilli(),
    anilistId = anilistId,
    malId = malId,
    syncedAtMs = null,
)

/**
 * BUG-144 parity: the title of the newest watch-history row of [ref]'s series (the one "Continuar viendo" shows), or null. History
 * rows key a series by its AniList id (`animeId` = the id) or by `mal:<id>`.
 */
internal suspend fun HistoryDao.newestTitle(ref: MediaRef): String? {
    val anilistId = ref.anilistId?.takeIf { it > 0 }
    val animeId = HistoryEntry.animeIdOf(anilistId, ref.malId) ?: return null
    return observeForAnime(anilistId ?: -1, animeId).first()
        .filter { it.title.isNotBlank() }
        .maxByOrNull { it.watchedAtMs }
        ?.title
}
