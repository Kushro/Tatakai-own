package app.tatakai.next.data

import app.tatakai.next.database.HistoryRow
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.supabase.RemoteWatchRow
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

internal fun HistoryRow.toRemote(): RemoteWatchRow = RemoteWatchRow(
    animeId = animeId,
    animeName = title.ifBlank { "Unknown Anime" },
    animePoster = poster ?: "",
    episodeId = episodeId,
    episodeNumber = episode,
    progressSeconds = progressSec,
    durationSeconds = durationSec,
    completed = completed,
    watchedAt = Instant.ofEpochMilli(watchedAtMs).toIsoMillis(),
    malId = malId,
    anilistId = anilistId,
)

internal fun RemoteWatchRow.toRow(): HistoryRow = HistoryRow(
    episodeId = episodeId,
    animeId = animeId,
    title = animeName,
    poster = animePoster?.takeIf { it.isNotEmpty() },
    episode = episodeNumber,
    progressSec = progressSeconds,
    durationSec = durationSeconds?.takeIf { it > 0 },
    completed = completed,
    watchedAtMs = parseInstant(watchedAt).toEpochMilli(),
    anilistId = anilistId,
    malId = malId,
    syncedAtMs = null,
)
