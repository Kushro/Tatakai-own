// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.AiringEpisode

/**
 * Parity wave B (16 §3.1, worker r, checklist home.calendar.schedule / home.latest-episodes): a provider that knows the airing schedule
 * (AniList `Page.airingSchedules`). Routed by [MetadataRouter.airingSchedule] through the enabled DISCOVER providers.
 */
interface AiringProvider {
    /** Episodes airing in `[fromEpochSec, toEpochSec)`, soonest first; [page] of `PER_PAGE_AIRING`. Second value: another page exists. */
    suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int): Pair<List<AiringEpisode>, Boolean>
}
