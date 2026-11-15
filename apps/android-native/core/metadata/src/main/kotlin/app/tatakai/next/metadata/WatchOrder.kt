// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRelation
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope

/**
 * Watch order of a franchise (classic `AnimeSeasons` / `useAnimeSeasons`): from [self], follow PREQUEL edges back to the first season,
 * then SEQUEL edges forward, anime only, each title once. [hop] returns the relations of one AniList id (one request); the walk stops
 * after [MAX_HOPS] in each direction so a huge franchise cannot drain the rate limit.
 */
object WatchOrder {
    const val MAX_HOPS = 12

    private fun List<MediaRelation>.next(type: String, seen: Set<Int>): Media? =
        firstOrNull { r ->
            r.relation == type && r.media.kind == MediaKind.ANIME && r.media.ref.anilistId?.let { it !in seen } == true
        }?.media

    suspend fun walk(self: Media, selfRelations: List<MediaRelation>, hop: suspend (Int) -> List<MediaRelation>): List<Media> {
        val selfId = self.ref.anilistId ?: return listOf(self)
        val seen = mutableSetOf(selfId)
        val before = ArrayDeque<Media>()
        var relations = selfRelations
        for (i in 0 until MAX_HOPS) {
            val prev = relations.next("PREQUEL", seen) ?: break
            val id = prev.ref.anilistId ?: break
            seen += id
            before.addFirst(prev)
            relations = hop(id)
        }
        val after = mutableListOf<Media>()
        relations = selfRelations
        for (i in 0 until MAX_HOPS) {
            val next = relations.next("SEQUEL", seen) ?: break
            val id = next.ref.anilistId ?: break
            seen += id
            after += next
            relations = hop(id)
        }
        return before.toList() + self + after
    }

    /**
     * 0.3.1: [walk] with the PREQUEL and SEQUEL chains followed side by side, so a batching [hop] sends the hops of the same depth
     * in one request. Same result as [walk] for linear franchises; in a loop (a title reached by both chains) the title stays on the
     * PREQUEL side only, like [walk], but the SEQUEL chain does not stop at it.
     */
    suspend fun walkConcurrently(self: Media, selfRelations: List<MediaRelation>, hop: suspend (Int) -> List<MediaRelation>): List<Media> {
        val selfId = self.ref.anilistId ?: return listOf(self)
        suspend fun chain(type: String): List<Media> {
            val seen = mutableSetOf(selfId)
            val out = mutableListOf<Media>()
            var relations = selfRelations
            for (i in 0 until MAX_HOPS) {
                val next = relations.next(type, seen) ?: break
                val id = next.ref.anilistId ?: break
                seen += id
                out += next
                relations = hop(id)
            }
            return out
        }
        val (before, after) = coroutineScope {
            val b = async { chain("PREQUEL") }
            val a = async { chain("SEQUEL") }
            b.await() to a.await()
        }
        val beforeIds = before.mapNotNull { it.ref.anilistId }.toSet()
        return before.reversed() + self + after.filter { it.ref.anilistId !in beforeIds }
    }
}
