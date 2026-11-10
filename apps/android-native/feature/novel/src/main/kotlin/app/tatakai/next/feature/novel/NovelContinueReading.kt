// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import app.tatakai.next.model.ContinueReadingItem
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.NovelProgress
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadPosition
import app.tatakai.next.novel.NovelRepositoryImpl

/** A saved novel paragraph as a [ReadPosition]; null when the media id has no AniList / MAL id. */
internal fun NovelProgress.toPosition(): ReadPosition? {
    val ref = NovelRepositoryImpl.refOf(mediaId) ?: return null
    return ReadPosition(mediaId, ref, title, poster, chapterNumber, offset >= NovelRepositoryImpl.COMPLETED_OFFSET, updatedAtMs)
}

/** "Seguir leyendo" of novels: the saved positions merged with the readlist entries being read (ListProgress.mergeContinueReading). */
internal fun novelContinueReading(progress: List<NovelProgress>, list: List<ReadEntry>, limit: Int): List<ContinueReadingItem> =
    ListProgress.mergeContinueReading(progress.mapNotNull { it.toPosition() }, list, limit)
