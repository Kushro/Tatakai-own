package app.tatakai.next.ui.media

import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs

/** Stable, unique-per-title key for `LazyList` items. */
fun mediaKey(media: Media): String = refKey(media.ref) ?: "t:${media.title}"

fun refKey(ref: MediaRef): String? =
    ref.anilistId?.let { "a$it" } ?: ref.malId?.let { "m$it" } ?: ref.kitsuId?.let { "k$it" }

/** Removes duplicates (same ref) that some feeds return across pages so list keys stay unique. */
fun List<Media>.distinctByKey(): List<Media> = distinctBy { mediaKey(it) }

/**
 * Player arguments to resume a history entry: same episode at the saved position, or the next episode from 0
 * when the entry is already completed (02 §4.1: "siguiente episodio").
 */
fun HistoryEntry.toResumeArgs(): PlayerArgs = PlayerArgs(
    media = MediaRef(anilistId = anilistId, malId = malId),
    title = title,
    episode = if (completed) episode + 1 else episode,
    startMs = if (completed) 0L else progressSec * 1000L,
)

/** Fraction watched in [0,1], or null when the duration is unknown. */
fun HistoryEntry.progressFraction(): Float? {
    val d = durationSec ?: return null
    if (d <= 0) return null
    return (progressSec.toFloat() / d).coerceIn(0f, 1f)
}
