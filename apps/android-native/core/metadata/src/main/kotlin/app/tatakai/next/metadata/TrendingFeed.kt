// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.MediaKind
import kotlinx.coroutines.CancellationException

/**
 * Parity wave A (worker o, checklist `search.feeds.trending`): the Tendencias feed of Search with the classic time window selector.
 * Not part of `MetadataRepository` (the contract keeps that interface fixed), so :feature:search reaches it through Koin. Failures are
 * thrown as [AppException] like the repository does; without an enabled DISCOVER provider the page is empty.
 */
class TrendingFeed(private val router: MetadataRouter) {
    suspend fun page(window: TrendWindow, page: Int, adult: Boolean, kind: MediaKind = MediaKind.ANIME): FeedPage = try {
        router.discover(kind, { it.items.isEmpty() }) { it.trending(kind, window, page, adult) }.value
    } catch (e: CancellationException) {
        throw e
    } catch (e: NoMetadataProviderException) {
        if (e.errors.isEmpty()) {
            FeedPage(emptyList(), page, false)
        } else {
            throw AppException(
                e.rateLimitedProvider?.let { AppError.RateLimited(it) } ?: if (e.allOffline) AppError.Offline else AppError.Unknown(e.message.orEmpty()),
                e,
            )
        }
    }
}
