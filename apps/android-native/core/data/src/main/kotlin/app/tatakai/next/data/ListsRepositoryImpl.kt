// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListConflict
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListSyncReport
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flowOf

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Worker q replaces the bodies: Room tables
 * `list_entry` / `list_change` (offline queue) / `list_conflict` / `linked_account` in :core:database (q owns it in this wave; bump the
 * version, destructive migration accepted as before), AniList GraphQL + MAL REST clients with the shared OkHttpClient, tokens sealed
 * under `noBackupFilesDir` (same AES/Keystore pattern as `ExtensionServerStore`), OAuth through `PendingOAuthRedirect`, client ids from
 * the Koin properties `PROP_ANILIST_CLIENT_ID` / `PROP_MAL_CLIENT_ID`. The constructor may grow (Koin `get()`s in `dataModule`).
 */
class ListsRepositoryImpl : ListsRepository {
    override val accounts: StateFlow<List<LinkedAccount>> = MutableStateFlow(emptyList())
    override val pendingChanges: StateFlow<Int> = MutableStateFlow(0)
    override val lastSyncedAt: StateFlow<Long?> = MutableStateFlow<Long?>(null)

    override fun entries(status: ListStatus?): Flow<List<ListEntry>> = flowOf(emptyList())

    override fun favorites(): Flow<List<ListEntry>> = flowOf(emptyList())

    override fun entry(ref: MediaRef): Flow<ListEntry?> = flowOf(null)

    override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> = TODO("worker q")

    override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> = TODO("worker q")

    override suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit> = TODO("worker q")

    override suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit> = TODO("worker q")

    override fun favoriteCharacters(): Flow<List<CharacterRef>> = flowOf(emptyList())

    override fun isCharacterFavorite(characterId: Int): Flow<Boolean> = flowOf(false)

    override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit> = TODO("worker q")

    override suspend fun beginLink(provider: ListProvider): Result<String> = TODO("worker q")

    override suspend fun completeLink(redirectUri: String): Result<LinkedAccount> = TODO("worker q")

    override suspend fun unlink(provider: ListProvider): Unit = TODO("worker q")

    override suspend fun syncNow(): Result<ListSyncReport> = TODO("worker q")

    override fun conflicts(): Flow<List<ListConflict>> = flowOf(emptyList())

    override suspend fun resolveConflict(id: String, choice: ConflictChoice): Unit = TODO("worker q")

    override suspend fun exportJson(): String = TODO("worker q")

    override suspend fun importJson(json: String): Result<Int> = TODO("worker q")
}
