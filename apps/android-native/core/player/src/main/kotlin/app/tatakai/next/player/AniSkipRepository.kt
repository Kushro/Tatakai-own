// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SkipMark
import app.tatakai.next.model.SkipMarksRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.TimeUnit

/**
 * [SkipMarksRepository] over AniSkip v2 (`https://api.aniskip.com/v2/skip-times/<mal>/<ep>?types=op&types=ed&types=recap&episodeLength=0`),
 * port of desktop `tk-app/src/live/torrent_play.rs::skip_marks`: fixed host, short timeout ([TIMEOUT_S]), cached per `(mal, episode)` for
 * the process. Only real answers are cached (200 and 404 = "nothing for this episode"); a network error is retried next time. Empty without
 * a MAL id. Never throws (except cancellation).
 */
class AniSkipRepository(baseClient: OkHttpClient, private val baseUrl: String = BASE_URL) : SkipMarksRepository {
    private val client = baseClient.newBuilder().callTimeout(TIMEOUT_S, TimeUnit.SECONDS).build()
    private val cache = ConcurrentHashMap<Pair<Int, Int>, List<SkipMark>>()

    override suspend fun skipMarks(media: MediaRef, episode: Int): List<SkipMark> {
        val mal = media.malId?.takeIf { it > 0 } ?: return emptyList()
        if (episode < 1) return emptyList()
        cache[mal to episode]?.let { return it }
        val url = "$baseUrl/$mal/$episode?types=op&types=ed&types=recap&episodeLength=0"
        return try {
            withContext(Dispatchers.IO) {
                client.newCall(Request.Builder().url(url).header("User-Agent", USER_AGENT).build()).execute().use { resp ->
                    when {
                        resp.isSuccessful -> parseAniSkip(resp.body.string()).also { cache[mal to episode] = it }
                        resp.code == 404 -> emptyList<SkipMark>().also { cache[mal to episode] = it }
                        else -> emptyList()
                    }
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            emptyList()
        }
    }

    companion object {
        const val BASE_URL = "https://api.aniskip.com/v2/skip-times"
        const val TIMEOUT_S = 6L
        private const val USER_AGENT = "TatakaiNext/0.1"
        private val json = Json { ignoreUnknownKeys = true; isLenient = true }

        /**
         * AniSkip v2 body (`{ found, results: [{ interval: { startTime, endTime }, skipType }] }`) -> marks sorted by start.
         * `op` / `mixed-op` = intro, `ed` / `mixed-ed` = outro, `recap`; anything else or malformed is skipped.
         */
        fun parseAniSkip(body: String): List<SkipMark> {
            val root = runCatching { json.parseToJsonElement(body) }.getOrNull() as? JsonObject ?: return emptyList()
            if ((root["found"] as? JsonPrimitive)?.booleanOrNull == false) return emptyList()
            val results = root["results"] as? JsonArray ?: return emptyList()
            return results.mapNotNull { r ->
                val o = r as? JsonObject ?: return@mapNotNull null
                val kind = when ((o["skipType"] as? JsonPrimitive)?.takeIf { it.isString }?.content) {
                    "op", "mixed-op" -> SkipKind.INTRO
                    "ed", "mixed-ed" -> SkipKind.OUTRO
                    "recap" -> SkipKind.RECAP
                    else -> return@mapNotNull null
                }
                val interval = o["interval"] as? JsonObject ?: return@mapNotNull null
                val start = (interval["startTime"] as? JsonPrimitive)?.doubleOrNull ?: return@mapNotNull null
                val end = (interval["endTime"] as? JsonPrimitive)?.doubleOrNull ?: return@mapNotNull null
                if (start.isFinite() && end.isFinite() && start >= 0.0 && end > start) SkipMark(kind, start, end) else null
            }.sortedBy { it.startSecs }
        }
    }
}
