// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.flow.StateFlow

/**
 * What the manga screens (:feature:manga) and the novel screens need beyond the FINAL `MangaRepository` of :core:model (16 §1: "si falta
 * algo, se añade privado en el módulo propio"). Implemented by [MangaRepositoryImpl]; bound in `mangaModule`.
 */
interface MangaCatalog {
    /** AniList facts of a manga / light novel (characters, staff, tags, relations, recommendations...). Cached for the session. */
    suspend fun info(ref: MediaRef): MangaInfo?

    /** MangaUpdates series of a title (publishers, serialization, status), null when not found. Cached on disk. */
    suspend fun mangaUpdates(ref: MediaRef): MangaUpdatesSeries?

    suspend fun genres(): List<String>

    /** Loads the 18+ zone of the manga home (only after the gate was accepted and `AppSettings.showAdult` is on). */
    suspend fun loadAdultZone()

    val prefs: StateFlow<MangaPrefs>

    suspend fun updatePrefs(transform: (MangaPrefs) -> MangaPrefs)

    /** Preferred chapter languages, best first (prefs + app language + English). */
    fun preferredLanguages(): List<String>

    /** The reader reports pages that failed / loaded so the source reliability follows reality. */
    suspend fun reportPageResult(sourceKey: String, ok: Boolean)

    /** Pulls the AniList manga list into the local readlist (Library › Manga refresh). */
    suspend fun pullReadlist(): Result<Int>

    /** The source a reader chapter belongs to (name and languages for the chrome); null when unknown. */
    suspend fun sourceOf(extensionId: String, sourceId: String, itemId: String): MangaChapterSource

    /** The chapter list of the source of a reader chapter (next / previous / chapter navigator). */
    suspend fun chaptersOf(extensionId: String, sourceId: String, itemId: String): List<MangaChapter>

    /** Removes a followed update from the home (opened / read). */
    suspend fun dismissUpdate(mediaId: String)
}
