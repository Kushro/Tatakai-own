// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.ListEntryRow
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import java.time.OffsetDateTime
import kotlin.math.roundToInt

// Pure mapping and merge rules of the lists layer (worker q, parity wave A §3.3). No Android, no network: unit-tested on the JVM.

internal val listJson = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = true
}

/** What both sides compare: the list status (null = not on the list), the episode progress and the 0..100 score. */
@Serializable
data class ListValues(val status: ListStatus? = null, val progress: Int = 0, val score: Int? = null)

fun ListEntry.values(): ListValues = ListValues(status, progress, score?.takeIf { it > 0 })

fun ListEntryRow.values(): ListValues = ListValues(statusOf(status), progress, score?.takeIf { it > 0 })

fun statusOf(id: String?): ListStatus? = id?.let { s -> ListStatus.entries.firstOrNull { it.id == s } }

/** Local key of a title: `HistoryEntry.animeIdOf` (`"21"`, `"mal:5114"`). */
fun keyOf(ref: MediaRef): String? = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)

// ------------------------------------------------------------------------------------------------ status mapping

/** AniList `MediaListStatus`: CURRENT, PLANNING, COMPLETED, DROPPED, PAUSED, REPEATING. */
fun ListStatus.toAniList(): String = when (this) {
    ListStatus.WATCHING -> "CURRENT"
    ListStatus.PLANNING -> "PLANNING"
    ListStatus.COMPLETED -> "COMPLETED"
    ListStatus.PAUSED -> "PAUSED"
    ListStatus.DROPPED -> "DROPPED"
    ListStatus.REPEATING -> "REPEATING"
}

fun aniListStatus(s: String?): ListStatus? = when (s?.uppercase()) {
    "CURRENT" -> ListStatus.WATCHING
    "PLANNING" -> ListStatus.PLANNING
    "COMPLETED" -> ListStatus.COMPLETED
    "PAUSED" -> ListStatus.PAUSED
    "DROPPED" -> ListStatus.DROPPED
    "REPEATING" -> ListStatus.REPEATING
    else -> null
}

/** MAL `my_list_status.status` + `is_rewatching` (MAL has no REPEATING: watching + is_rewatching, decision 15 §7.9). */
fun ListStatus.toMal(): Pair<String, Boolean> = when (this) {
    ListStatus.WATCHING -> "watching" to false
    ListStatus.REPEATING -> "watching" to true
    ListStatus.PLANNING -> "plan_to_watch" to false
    ListStatus.COMPLETED -> "completed" to false
    ListStatus.PAUSED -> "on_hold" to false
    ListStatus.DROPPED -> "dropped" to false
}

fun malStatus(s: String?, rewatching: Boolean): ListStatus? = when (s?.lowercase()) {
    "watching" -> if (rewatching) ListStatus.REPEATING else ListStatus.WATCHING
    "completed" -> if (rewatching) ListStatus.REPEATING else ListStatus.COMPLETED
    "on_hold" -> ListStatus.PAUSED
    "dropped" -> ListStatus.DROPPED
    "plan_to_watch" -> ListStatus.PLANNING
    else -> null
}

// ------------------------------------------------------------------------------------------------ score formats

/**
 * Granularity of a provider score format on the 0..100 scale, so `85` and an AniList POINT_10 `80` are "the same" only when the
 * account cannot store the difference. MAL is 0..10 (step 10). AniList: POINT_100 / POINT_10_DECIMAL 1, POINT_10 10, POINT_5 20,
 * POINT_3 ~33.
 */
fun scoreStep(provider: ListProvider, format: String?): Int = when {
    provider == ListProvider.MAL -> 10
    format == "POINT_10" -> 10
    format == "POINT_5" -> 20
    format == "POINT_3" -> 34
    else -> 1
}

/** 0..100 -> MAL 0..10. */
fun toMalScore(score: Int?): Int = ((score ?: 0) / 10.0).roundToInt().coerceIn(0, 10)

/** Human score in the account's format: `8.5`, `85`, `4★`, `:)`-less 3-point shown as `1..3`. */
fun formatScore(score: Int, format: String?): String = when (format) {
    "POINT_100" -> score.toString()
    "POINT_10" -> (score / 10.0).roundToInt().toString()
    "POINT_5" -> "${(score / 20.0).roundToInt()}★"
    "POINT_3" -> (score / 33.4).roundToInt().coerceIn(1, 3).toString()
    "mal" -> (score / 10.0).roundToInt().toString()
    else -> if (score % 10 == 0) (score / 10).toString() else String.format(java.util.Locale.ROOT, "%.1f", score / 10.0)
}

fun sameValues(a: ListValues?, b: ListValues?, step: Int = 1): Boolean {
    val x = a?.takeIf { it.status != null }
    val y = b?.takeIf { it.status != null }
    if (x == null || y == null) return x == null && y == null
    if (x.status != y.status || x.progress != y.progress) return false
    val sx = x.score ?: 0
    val sy = y.score ?: 0
    return if (step <= 1) sx == sy else (sx.toDouble() / step).roundToInt() == (sy.toDouble() / step).roundToInt()
}

// ------------------------------------------------------------------------------------------------ merge rule

enum class InitialSyncMode {
    /** Default: both sides kept, disagreements become conflicts. */
    MERGE,

    /** Remote overwrites local where both exist; local-only titles stay local. */
    IMPORT,

    /** Local overwrites remote; remote-only titles are still pulled. */
    EXPORT,
}

enum class SyncAction { NOOP, PUSH, PULL, ADOPT, CONFLICT }

/**
 * Three-way decision for one title and one provider (classic `listSync`): [local] / [remote] values (status null = absent), [base] = the
 * values both agreed on after the last sync (null = never synced), [queued] = a local change waits in the offline queue.
 * PUSH with an absent local deletes remotely; PULL with an absent remote removes locally; ADOPT = equal now, only record the base.
 */
fun decideSync(
    local: ListValues?,
    remote: ListValues?,
    base: ListValues?,
    queued: Boolean,
    step: Int = 1,
    mode: InitialSyncMode = InitialSyncMode.MERGE,
): SyncAction {
    val l = local?.takeIf { it.status != null }
    val r = remote?.takeIf { it.status != null }
    val b = base?.takeIf { it.status != null }
    val localEqRemote = sameValues(l, r, step)
    if (queued) {
        return when {
            localEqRemote -> SyncAction.ADOPT
            base == null && r == null -> SyncAction.PUSH
            base != null && sameValues(r, b, step) -> SyncAction.PUSH
            mode == InitialSyncMode.EXPORT -> SyncAction.PUSH
            else -> SyncAction.CONFLICT
        }
    }
    if (base == null) {
        return when {
            l == null && r == null -> SyncAction.NOOP
            localEqRemote -> SyncAction.ADOPT
            r == null -> if (mode == InitialSyncMode.IMPORT) SyncAction.NOOP else SyncAction.PUSH
            l == null -> SyncAction.PULL
            mode == InitialSyncMode.IMPORT -> SyncAction.PULL
            mode == InitialSyncMode.EXPORT -> SyncAction.PUSH
            else -> SyncAction.CONFLICT
        }
    }
    val remoteChanged = !sameValues(r, b, step)
    val localChanged = !sameValues(l, b, step)
    return when {
        !remoteChanged && !localChanged -> SyncAction.NOOP
        !remoteChanged -> SyncAction.PUSH
        !localChanged -> SyncAction.PULL
        localEqRemote -> SyncAction.ADOPT
        else -> SyncAction.CONFLICT
    }
}

// ------------------------------------------------------------------------------------------------ per-provider base snapshots

private val BASES = MapSerializer(String.serializer(), ListValues.serializer())

fun ListEntryRow.bases(): Map<String, ListValues> =
    remoteJson?.let { runCatching { listJson.decodeFromString(BASES, it) }.getOrNull() }.orEmpty()

fun ListEntryRow.base(provider: ListProvider): ListValues? = bases()[provider.id]

fun ListEntryRow.withBase(provider: ListProvider, values: ListValues?): ListEntryRow {
    val next = bases().toMutableMap()
    if (values == null) next.remove(provider.id) else next[provider.id] = values
    return copy(remoteJson = if (next.isEmpty()) null else listJson.encodeToString(BASES, next))
}

// ------------------------------------------------------------------------------------------------ row <-> model

fun ListEntryRow.toEntry(): ListEntry? {
    val st = statusOf(status) ?: return null
    return ListEntry(
        ref = MediaRef(anilistId = anilistId, malId = malId),
        title = title,
        poster = poster,
        status = st,
        progress = progress,
        totalEpisodes = totalEpisodes,
        score = score,
        favorite = favorite,
        updatedAtMs = updatedAtMs,
        notes = notes,
        startedAt = startedAt,
        completedAt = completedAt,
    )
}

/** Favorites may be off the list (status null): shown with PLANNING as a neutral status (the Favorites tab ignores the status). */
fun ListEntryRow.toFavoriteEntry(): ListEntry = toEntry() ?: ListEntry(
    ref = MediaRef(anilistId = anilistId, malId = malId),
    title = title,
    poster = poster,
    status = ListStatus.PLANNING,
    progress = progress,
    totalEpisodes = totalEpisodes,
    score = score,
    favorite = favorite,
    updatedAtMs = updatedAtMs,
)

fun ListEntry.toRow(key: String, previous: ListEntryRow? = null): ListEntryRow = ListEntryRow(
    key = key,
    anilistId = ref.anilistId ?: previous?.anilistId,
    malId = ref.malId ?: previous?.malId,
    title = title.ifBlank { previous?.title.orEmpty() },
    poster = poster ?: previous?.poster,
    status = status.id,
    progress = progress.coerceAtLeast(0),
    totalEpisodes = totalEpisodes ?: previous?.totalEpisodes,
    score = score?.coerceIn(0, 100)?.takeIf { it > 0 },
    favorite = favorite,
    updatedAtMs = updatedAtMs,
    notes = notes,
    startedAt = startedAt,
    completedAt = completedAt,
    remoteJson = previous?.remoteJson,
)

/** Applies remote [values] (and the provider's title data) onto a local row, keeping local-only fields. */
fun ListEntryRow.applyRemote(remote: ListEntry?, nowMs: Long): ListEntryRow = if (remote == null) {
    copy(status = null, updatedAtMs = nowMs)
} else {
    copy(
        anilistId = anilistId ?: remote.ref.anilistId,
        malId = malId ?: remote.ref.malId,
        title = title.ifBlank { remote.title },
        poster = poster ?: remote.poster,
        status = remote.status.id,
        progress = remote.progress,
        totalEpisodes = remote.totalEpisodes ?: totalEpisodes,
        score = remote.score?.takeIf { it > 0 },
        updatedAtMs = maxOf(nowMs, remote.updatedAtMs),
        startedAt = remote.startedAt ?: startedAt,
        completedAt = remote.completedAt ?: completedAt,
    )
}

// ------------------------------------------------------------------------------------------------ import / export (desktop format)

/** JSON array of [ListEntry] (camelCase, lowercase ids): the desktop import / export format. */
fun exportEntries(entries: List<ListEntry>): String =
    listJson.encodeToString(kotlinx.serialization.builtins.ListSerializer(ListEntry.serializer()), entries)

/** Parses an export; entries without any id are dropped. Accepts a bare array or `{ "entries": [...] }`. */
fun parseExport(json: String): List<ListEntry> {
    val element = listJson.parseToJsonElement(json)
    val array = when (element) {
        is kotlinx.serialization.json.JsonArray -> element
        is JsonObject -> element["entries"] as? kotlinx.serialization.json.JsonArray ?: throw IllegalArgumentException("no entries")
        else -> throw IllegalArgumentException("not a list export")
    }
    return array.mapNotNull { e ->
        runCatching { listJson.decodeFromJsonElement(ListEntry.serializer(), e) }.getOrNull()?.takeIf { keyOf(it.ref) != null }
    }
}

/** Import rule: a new title is added, an existing one is replaced only when the imported `updatedAtMs` is newer. */
fun shouldImport(existing: ListEntryRow?, incoming: ListEntry): Boolean =
    existing == null || existing.status == null || incoming.updatedAtMs > existing.updatedAtMs

// ------------------------------------------------------------------------------------------------ provider answers

/** AniList `MediaList` (with `media { … }`) -> [ListEntry]. Score requested as `score(format: POINT_100)`. */
fun parseAniListEntry(o: JsonObject): ListEntry? {
    val media = o.obj("media")
    val id = o.int("mediaId") ?: media?.int("id") ?: return null
    val status = aniListStatus(o.str("status")) ?: return null
    val title = media?.obj("title")?.let { t -> t.str("userPreferred") ?: t.str("romaji") ?: t.str("english") ?: t.str("native") }
    return ListEntry(
        ref = MediaRef(anilistId = id, malId = media?.int("idMal")),
        title = title ?: "#$id",
        poster = media?.obj("coverImage")?.let { it.str("large") ?: it.str("medium") },
        status = status,
        progress = o.int("progress") ?: 0,
        totalEpisodes = media?.int("episodes"),
        score = o.int("score")?.takeIf { it > 0 }?.coerceIn(0, 100),
        favorite = media?.bool("isFavourite") ?: false,
        updatedAtMs = (o.long("updatedAt") ?: 0L) * 1000,
        notes = o.str("notes")?.takeIf { it.isNotBlank() },
        startedAt = o.obj("startedAt")?.let(::fuzzyDate),
        completedAt = o.obj("completedAt")?.let(::fuzzyDate),
    )
}

/** Every entry of a `MediaListCollection` answer (`data.MediaListCollection.lists[].entries[]`), de-duplicated by media id. */
fun parseAniListCollection(data: JsonObject): Pair<List<ListEntry>, Boolean> {
    val collection = data.obj("MediaListCollection") ?: return emptyList<ListEntry>() to false
    val byId = LinkedHashMap<Int, ListEntry>()
    collection.arr("lists")?.objectList()?.forEach { list ->
        list.arr("entries")?.objectList()?.forEach { e ->
            parseAniListEntry(e)?.let { entry -> entry.ref.anilistId?.let { byId.putIfAbsent(it, entry) } }
        }
    }
    return byId.values.toList() to (collection.bool("hasNextChunk") == true)
}

private fun fuzzyDate(o: JsonObject): String? {
    val y = o.int("year") ?: return null
    val m = o.int("month") ?: return y.toString()
    val d = o.int("day") ?: return "%04d-%02d".format(y, m)
    return "%04d-%02d-%02d".format(y, m, d)
}

/** MAL `/users/@me/animelist?fields=list_status,num_episodes` item (`{ node: {…}, list_status: {…} }`) -> [ListEntry]. */
fun parseMalItem(item: JsonObject): ListEntry? {
    val node = item.obj("node") ?: return null
    val id = node.int("id") ?: return null
    val ls = item.obj("list_status") ?: node.obj("my_list_status") ?: return null
    val status = malStatus(ls.str("status"), ls.bool("is_rewatching") == true) ?: return null
    return ListEntry(
        ref = MediaRef(malId = id),
        title = node.str("title") ?: "#$id",
        poster = node.obj("main_picture")?.let { it.str("large") ?: it.str("medium") },
        status = status,
        progress = ls.int("num_episodes_watched") ?: 0,
        totalEpisodes = node.int("num_episodes")?.takeIf { it > 0 },
        score = ls.int("score")?.takeIf { it > 0 }?.let { (it * 10).coerceIn(0, 100) },
        favorite = false,
        updatedAtMs = ls.str("updated_at")?.let { runCatching { OffsetDateTime.parse(it).toInstant().toEpochMilli() }.getOrNull() } ?: 0L,
        notes = ls.str("comments")?.takeIf { it.isNotBlank() },
        startedAt = ls.str("start_date"),
        completedAt = ls.str("finish_date"),
    )
}

/** A MAL list page: items and the `paging.next` URL when it stays on `api.myanimelist.net` (never send the token elsewhere). */
fun parseMalPage(body: JsonObject): Pair<List<ListEntry>, String?> {
    val items = body.arr("data")?.objectList()?.mapNotNull(::parseMalItem).orEmpty()
    val next = body.obj("paging")?.str("next")?.takeIf { it.startsWith("https://api.myanimelist.net/") }
    return items to next
}

/** Decodes a JSON object answer (helper for the clients and the tests). */
fun jsonObjectOf(text: String): JsonObject = listJson.parseToJsonElement(text).jsonObject
