// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Media
import app.tatakai.next.model.SourceCandidate
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf

/**
 * Source candidates that do not come from an installed `.kai` extension: today the linked PC extension servers
 * (`:core:extension-server`, docs/redesign/13 §3.10). `ExtensionRepositoryImpl.resolveCandidates` asks every source in parallel with the
 * installed extensions and ranks everything together.
 *
 * Implementations bound their own time (a down server must not delay the local extensions beyond its timeout), never throw except
 * for cancellation, and use `extId`s that cannot collide with extension ids and contain no `|` (it separates `sourceKey`).
 */
interface ExternalCandidateSource {
    suspend fun candidates(media: Media, episode: Int): List<SourceCandidate>

    /** Whether this source currently has something to ask (e.g. at least one enabled linked server). */
    val available: Flow<Boolean> get() = flowOf(true)
}
