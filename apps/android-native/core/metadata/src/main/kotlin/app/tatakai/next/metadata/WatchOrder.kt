// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRelation

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
}
