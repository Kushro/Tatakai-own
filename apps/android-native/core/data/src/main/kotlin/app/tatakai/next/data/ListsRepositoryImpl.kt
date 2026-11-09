// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.database.FavoriteCharacterRow
import app.tatakai.next.database.LinkedAccountRow
import app.tatakai.next.database.ListChangeRow
import app.tatakai.next.database.ListConflictRow
import app.tatakai.next.database.ListEntryRow
import app.tatakai.next.database.ListsDao
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppLog
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
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.time.LocalDate
import java.util.concurrent.TimeUnit

/** Extra controls of the lists sync beyond the contract (Settings › Listas y cuentas: first sync mode, queue flush). */
interface ListsSyncControl {
    /** One provider, with the first-sync choice of `lists.import-export` (import remote / export local / merge). */
    suspend fun syncWith(provider: ListProvider, mode: InitialSyncMode): Result<ListSyncReport>

    /** Push the offline queue only (no full pull). */
    suspend fun pushPending(): ListSyncReport

    /** Score format of the account used to show / pick scores ("POINT_10_DECIMAL", "POINT_5", "mal"...); null without accounts. */
    val scoreFormat: StateFlow<String?>

    /**
     * Linked providers whose first sync waits for the user's choice (`lists.import-export`): set by a link while the local list has
     * entries ([needsFirstSyncChoice]), restored after a restart ([pendingFirstSyncs]), cleared by a successful [syncWith] or an unlink.
     * [ListsRepository.syncNow] and [pushPending] leave these providers alone until then, so nothing is merged behind the user's back.
     */
    val firstSyncPending: StateFlow<Set<ListProvider>>
}

/** Asks for a push of the offline queue as soon as there is a network (WorkManager, survives process death). */
fun interface ListSyncScheduler {
    fun requestPush()
}

class WorkManagerListSyncScheduler(private val context: Context) : ListSyncScheduler {
    override fun requestPush() {
        val request = OneTimeWorkRequestBuilder<ListSyncWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .build()
        // REPLACE: a new change must not wait behind a backed-off attempt; the queue in Room is the source of truth.
        WorkManager.getInstance(context).enqueueUniqueWork(ListSyncWorker.UNIQUE_NAME, ExistingWorkPolicy.REPLACE, request)
    }
}

class ListSyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val lists: ListsRepository by inject()

    override suspend fun doWork(): Result = try {
        val control = lists as? ListsSyncControl
        if (control == null) {
            Result.success()
        } else {
            val report = control.pushPending()
            // offline / provider down: keep the queue and retry with backoff
            if (report.failed > 0 && lists.pendingChanges.value > 0) Result.retry() else Result.success()
        }
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME = "tatakai-lists-push"
    }
}

/**
 * Lists, favorites and linked accounts (contract 15 §3.3; desktop `tk-core/lists`, classic `useWatchlist` + `listSync`). Local first:
 * every write lands in Room at once and, for each linked provider, a coalesced change enters the offline queue (`list_change`), pushed by
 * [ListSyncWorker] when a network exists. Before a push the remote values of that title are read: if they changed since the last agreed
 * values (per-provider base snapshot) and differ from the local ones, a [ListConflict] is recorded instead of overwriting.
 * [syncNow] pulls the whole remote lists with the same three-way rule ([decideSync]). Tokens live in [secrets] only.
 */
class ListsRepositoryImpl(
    private val dao: ListsDao,
    private val secrets: SecretStore,
    private val anilist: AniListListClient,
    private val mal: MalListClient,
    private val clientIds: (ListProvider) -> String?,
    private val scheduler: ListSyncScheduler? = null,
    private val clock: () -> Long = System::currentTimeMillis,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
    /** Sync once shortly after creation (app open) when an account is linked. */
    syncOnStart: Boolean = true,
) : ListsRepository, ListsSyncControl {

    private val syncMutex = Mutex()

    private val accountRows: StateFlow<List<LinkedAccountRow>> =
        dao.observeAccounts().stateIn(scope, SharingStarted.Eagerly, emptyList())

    override val accounts: StateFlow<List<LinkedAccount>> = accountRows
        .map { rows -> rows.mapNotNull { it.toModel() } }
        .stateIn(scope, SharingStarted.Eagerly, emptyList())

    override val pendingChanges: StateFlow<Int> = dao.observeChangeCount().stateIn(scope, SharingStarted.Eagerly, 0)

    override val lastSyncedAt: StateFlow<Long?> = accountRows
        .map { rows -> rows.mapNotNull { it.lastSyncedAtMs }.maxOrNull() }
        .stateIn(scope, SharingStarted.Eagerly, null)

    override val scoreFormat: StateFlow<String?> = accountRows
        .map { rows -> (rows.firstOrNull { it.provider == ListProvider.ANILIST.id } ?: rows.firstOrNull())?.scoreFormat }
        .stateIn(scope, SharingStarted.Eagerly, null)

    private val _firstSyncPending = MutableStateFlow<Set<ListProvider>>(emptySet())
    override val firstSyncPending: StateFlow<Set<ListProvider>> = _firstSyncPending.asStateFlow()

    /** Restores [firstSyncPending] from Room once; the start sync waits for it so it never merges a pending provider. */
    private val pendingRestored = scope.async {
        runCatching { pendingFirstSyncs(dao.accounts(), dao.all()) }.getOrDefault(emptySet())
            .also { restored -> _firstSyncPending.update { it + restored } }
    }

    init {
        if (syncOnStart) {
            scope.launch {
                pendingRestored.await()
                delay(START_SYNC_DELAY_MS)
                if (dao.accounts().isNotEmpty()) runCatching { syncNow() }
            }
        }
    }

    // ------------------------------------------------------------------------------------------------ reads

    override fun entries(status: ListStatus?): Flow<List<ListEntry>> =
        (if (status == null) dao.observeListed() else dao.observeByStatus(status.id)).map { rows -> rows.mapNotNull { it.toEntry() } }

    override fun favorites(): Flow<List<ListEntry>> = dao.observeFavorites().map { rows -> rows.map { it.toFavoriteEntry() } }

    override fun entry(ref: MediaRef): Flow<ListEntry?> {
        val key = keyOf(ref) ?: return kotlinx.coroutines.flow.flowOf(null)
        return dao.observe(key).map { it?.toEntry() }.distinctUntilChanged()
    }

    override fun favoriteCharacters(): Flow<List<CharacterRef>> =
        dao.observeVisibleCharacters().map { rows -> rows.map { CharacterRef(it.id, it.name, it.image) } }

    override fun isCharacterFavorite(characterId: Int): Flow<Boolean> =
        dao.observeVisibleCharacterCount(characterId).map { it > 0 }.distinctUntilChanged()

    // ------------------------------------------------------------------------------------------------ writes (local first)

    override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> = write {
        val key = keyOf(media.ref) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
        val prev = dao.get(key)
        val now = clock()
        if (status == null) {
            if (prev == null || prev.status == null) return@write
            store(prev.copy(status = null, updatedAtMs = now), queue = OP_SAVE)
            return@write
        }
        val base = prev ?: ListEntryRow(
            key = key,
            anilistId = media.ref.anilistId,
            malId = media.ref.malId,
            title = media.title,
            poster = media.cover,
            status = null,
            totalEpisodes = media.episodes,
            updatedAtMs = now,
        )
        val total = base.totalEpisodes ?: media.episodes
        val today = LocalDate.now().toString()
        val row = base.copy(
            anilistId = base.anilistId ?: media.ref.anilistId,
            malId = base.malId ?: media.ref.malId,
            title = base.title.ifBlank { media.title },
            poster = base.poster ?: media.cover,
            totalEpisodes = total,
            status = status.id,
            progress = if (status == ListStatus.COMPLETED && total != null && total > 0) total else base.progress,
            startedAt = base.startedAt ?: today.takeIf { status == ListStatus.WATCHING || status == ListStatus.COMPLETED },
            completedAt = if (status == ListStatus.COMPLETED) base.completedAt ?: today else base.completedAt,
            updatedAtMs = now,
        )
        store(row, queue = OP_SAVE)
    }

    override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> = write {
        val key = keyOf(ref) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
        val prev = dao.get(key)?.takeIf { it.status != null } ?: throw AppException(AppError.Unknown(ListErrors.NOT_ON_LIST))
        store(progressed(prev, episode), queue = OP_SAVE)
    }

    override suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit> = write {
        val key = keyOf(ref) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
        val prev = dao.get(key)?.takeIf { it.status != null } ?: throw AppException(AppError.Unknown(ListErrors.NOT_ON_LIST))
        store(prev.copy(score = score?.coerceIn(0, 100)?.takeIf { it > 0 }, updatedAtMs = clock()), queue = OP_SAVE)
    }

    override suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit> = write {
        val key = keyOf(media.ref) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
        val prev = dao.get(key)
        if (prev == null && !favorite) return@write
        val row = (prev ?: ListEntryRow(
            key = key, anilistId = media.ref.anilistId, malId = media.ref.malId, title = media.title, poster = media.cover,
            status = null, totalEpisodes = media.episodes, updatedAtMs = clock(),
        )).copy(favorite = favorite, updatedAtMs = clock())
        dao.upsert(row)
        if (row.anilistId != null && isLinked(ListProvider.ANILIST)) {
            dao.putChange(ListChangeRow(ListProvider.ANILIST.id, key, OP_FAVORITE, favorite = favorite, createdAtMs = clock()))
            scheduler?.requestPush()
        }
        cleanup(row)
    }

    override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit> = write {
        val linked = isLinked(ListProvider.ANILIST)
        val prev = dao.character(character.id)
        when {
            favorite -> dao.putCharacter(
                FavoriteCharacterRow(character.id, character.name, character.image, prev?.addedAtMs ?: clock(), pendingRemote = if (linked) true else null),
            )
            linked -> dao.putCharacter(
                (prev ?: FavoriteCharacterRow(character.id, character.name, character.image, clock())).copy(pendingRemote = false),
            )
            else -> dao.deleteCharacter(character.id)
        }
        if (linked) scheduler?.requestPush()
    }

    override suspend fun onEpisodeCompleted(ref: MediaRef, episode: Int) {
        val key = keyOf(ref) ?: return
        val prev = dao.get(key) ?: return
        val status = statusOf(prev.status) ?: return
        if (status == ListStatus.COMPLETED || status == ListStatus.DROPPED) return
        if (episode <= prev.progress) return
        runCatching { store(progressed(prev, episode), queue = OP_SAVE) }
    }

    /** PLANNING / PAUSED -> WATCHING on progress; WATCHING / REPEATING -> COMPLETED at the total. */
    private fun progressed(prev: ListEntryRow, episode: Int): ListEntryRow {
        val total = prev.totalEpisodes?.takeIf { it > 0 }
        val ep = if (total != null) episode.coerceIn(0, total) else episode.coerceAtLeast(0)
        var status = statusOf(prev.status) ?: ListStatus.WATCHING
        if (ep > 0 && (status == ListStatus.PLANNING || status == ListStatus.PAUSED)) status = ListStatus.WATCHING
        val done = total != null && ep >= total && ep > 0
        if (done && (status == ListStatus.WATCHING || status == ListStatus.REPEATING)) status = ListStatus.COMPLETED
        val today = LocalDate.now().toString()
        return prev.copy(
            progress = ep,
            status = status.id,
            startedAt = prev.startedAt ?: today.takeIf { ep > 0 },
            completedAt = if (status == ListStatus.COMPLETED) prev.completedAt ?: today else prev.completedAt,
            updatedAtMs = clock(),
        )
    }

    /** Stores [row] and queues it for every linked provider. */
    private suspend fun store(row: ListEntryRow, queue: String) {
        dao.upsert(row)
        val linked = dao.accounts()
        linked.forEach { a -> dao.putChange(ListChangeRow(a.provider, row.key, queue, createdAtMs = clock())) }
        if (linked.isNotEmpty()) scheduler?.requestPush()
        cleanup(row)
    }

    /** Off the list, not a favorite, never synced anywhere: nothing to remember. */
    private suspend fun cleanup(row: ListEntryRow) {
        if (row.status == null && !row.favorite && row.bases().isEmpty() && dao.accounts().isEmpty()) dao.delete(row.key)
    }

    private suspend fun write(block: suspend () -> Unit): Result<Unit> = try {
        block()
        Result.success(Unit)
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }

    // ------------------------------------------------------------------------------------------------ OAuth

    override suspend fun beginLink(provider: ListProvider): Result<String> {
        val clientId = clientIds(provider)?.trim()?.takeIf { it.isNotEmpty() }
            ?: return Result.failure(AppException(AppError.Unknown("${ListErrors.NO_CLIENT_ID}:${provider.id}")))
        val state = Pkce.state()
        val verifier = if (provider == ListProvider.MAL) Pkce.verifier() else null
        secrets.put(KEY_PENDING, listJson.encodeToString(PendingOAuth.serializer(), PendingOAuth(provider.id, state, verifier, clock())))
        return Result.success(
            when (provider) {
                ListProvider.ANILIST -> AniListListClient.authorizeUrl(clientId, state)
                ListProvider.MAL -> MalListClient.authorizeUrl(clientId, state, verifier!!)
            },
        )
    }

    override suspend fun completeLink(redirectUri: String): Result<LinkedAccount> = try {
        Result.success(finishLink(redirectUri))
    } catch (e: CancellationException) {
        throw e
    } catch (e: AppException) {
        AppLog.w(TAG, "link failed: ${e.error}")
        Result.failure(e)
    } catch (e: Exception) {
        Result.failure(AppException(AppError.Unknown(ListErrors.EXCHANGE), e))
    }

    private suspend fun finishLink(redirectUri: String): LinkedAccount {
        val provider = OAuthRedirect.providerOf(redirectUri) ?: throw AppException(AppError.Unknown(ListErrors.INVALID_STATE))
        val params = OAuthRedirect.params(redirectUri)
        val pending = secrets.get(KEY_PENDING)?.let { runCatching { listJson.decodeFromString(PendingOAuth.serializer(), it) }.getOrNull() }
        params["error"]?.let { error ->
            if (pending?.provider == provider.id) secrets.put(KEY_PENDING, null)
            throw AppException(AppError.Unknown(if (error == "access_denied") ListErrors.CANCELLED else "${ListErrors.DENIED}:$error"))
        }
        val state = params["state"]
        val valid = pending != null && pending.provider == provider.id && !pending.isExpired(clock()) &&
            (state == null || state == pending.state) &&
            // the authorization-code redirects must echo the state
            (state != null || params["access_token"] != null)
        if (!valid) throw AppException(AppError.Unknown(ListErrors.INVALID_STATE))
        val clientId = clientIds(provider)?.trim()?.takeIf { it.isNotEmpty() }
            ?: throw AppException(AppError.Unknown("${ListErrors.NO_CLIENT_ID}:${provider.id}"))
        val redirect = OAuthRedirect.redirectUri(provider)
        val tokens = when (provider) {
            ListProvider.ANILIST -> params["access_token"]?.let { access ->
                TokenSet(access = access, expiresAtMs = params["expires_in"]?.toLongOrNull()?.let { clock() + it * 1000 })
            } ?: params["code"]?.let { anilist.exchangeCode(clientId, it, redirect) }
            ListProvider.MAL -> params["code"]?.let { code ->
                val verifier = pending!!.verifier ?: throw AppException(AppError.Unknown(ListErrors.INVALID_STATE))
                mal.exchangeCode(clientId, code, verifier, redirect)
            }
        } ?: throw AppException(AppError.Unknown(ListErrors.CANCELLED))
        secrets.put(KEY_PENDING, null)
        val account = try {
            remote(provider).viewer(tokens.access)
        } catch (e: AppException) {
            if ((e.error as? AppError.Http)?.code == 401) throw AppException(AppError.Unknown(ListErrors.EXPIRED), e) else throw e
        }
        secrets.put(SecretKeys.listToken(provider.id), listJson.encodeToString(TokenSet.serializer(), tokens))
        val row = LinkedAccountRow(
            provider = provider.id,
            userId = account.userId,
            username = account.username,
            avatar = account.avatar,
            linkedAtMs = clock(),
            expiresAtMs = tokens.expiresAtMs,
            scoreFormat = account.scoreFormat,
        )
        pendingRestored.await()
        dao.putAccount(row)
        AppLog.i(TAG, "linked ${provider.id}")
        if (needsFirstSyncChoice(dao.all())) {
            // lists.import-export: the user picks import remote / export local / merge (Settings › Listas y cuentas sheet)
            _firstSyncPending.update { it + provider }
        } else {
            // nothing local: every mode pulls the same, so merge right away
            _firstSyncPending.update { it - provider }
            scope.launch { runCatching { syncWith(provider, InitialSyncMode.MERGE) } }
        }
        return row.toModel()!!
    }

    override suspend fun unlink(provider: ListProvider) {
        _firstSyncPending.update { it - provider }
        secrets.put(SecretKeys.listToken(provider.id), null)
        dao.deleteAccount(provider.id)
        dao.deleteChangesOf(provider.id)
        dao.deleteConflictsOf(provider.id)
        val stillLinked = dao.accounts().isNotEmpty()
        for (row in dao.all()) {
            val base = row.base(provider) ?: continue
            val pulledOnly = row.bases().keys == setOf(provider.id) && sameValues(row.values(), base)
            val next = row.withBase(provider, null)
            if (pulledOnly && !row.favorite) {
                // came from that account and was never touched here: the remote list stops showing
                dao.delete(row.key)
            } else {
                dao.upsert(next)
                if (!stillLinked && next.status == null && !next.favorite) dao.delete(row.key)
            }
        }
        if (provider == ListProvider.ANILIST) {
            dao.observeCharacters().first().filter { it.pendingRemote == false }.forEach { dao.deleteCharacter(it.id) }
            dao.observeCharacters().first().filter { it.pendingRemote == true }.forEach { dao.putCharacter(it.copy(pendingRemote = null)) }
        }
        AppLog.i(TAG, "unlinked ${provider.id}")
    }

    // ------------------------------------------------------------------------------------------------ sync

    override suspend fun syncNow(): Result<ListSyncReport> {
        val rows = dao.accounts()
        if (rows.isEmpty()) return Result.success(ListSyncReport(0, 0, 0, 0))
        var total = ListSyncReport(0, 0, 0, 0)
        var firstError: Throwable? = null
        for (row in rows) {
            val provider = ListProvider.entries.firstOrNull { it.id == row.provider } ?: continue
            // first sync not chosen yet (lists.import-export): wait for the user's import / export / merge choice
            if (provider in _firstSyncPending.value) continue
            syncWith(provider, InitialSyncMode.MERGE).fold(
                onSuccess = { total = total + it },
                onFailure = { if (firstError == null) firstError = it },
            )
        }
        return firstError?.let { Result.failure(it) } ?: Result.success(total)
    }

    override suspend fun syncWith(provider: ListProvider, mode: InitialSyncMode): Result<ListSyncReport> = try {
        Result.success(syncMutex.withLock { syncProvider(provider, mode) }).also { _firstSyncPending.update { it - provider } }
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        AppLog.w(TAG, "sync ${provider.id} failed: ${(e as? AppException)?.error ?: e::class.java.simpleName}")
        Result.failure(e)
    }

    private suspend fun syncProvider(provider: ListProvider, mode: InitialSyncMode): ListSyncReport {
        val account = dao.accounts().firstOrNull { it.provider == provider.id } ?: throw AppException(AppError.Unknown(ListErrors.NOT_LINKED))
        val api = remote(provider)
        val step = scoreStep(provider, account.scoreFormat)
        val remoteAccount = RemoteAccount(account.userId, account.username, account.avatar, account.scoreFormat)
        val remoteList = withToken(provider) { api.fetchAll(it, remoteAccount) }
        val queued = dao.changes().filter { it.provider == provider.id && it.op == OP_SAVE }.map { it.key }.toSet()

        // map every remote entry onto a local key (provider id first, then the generic key)
        val remoteByKey = LinkedHashMap<String, ListEntry>()
        for (r in remoteList) {
            val id = api.idOf(r) ?: continue
            val existing = when (provider) {
                ListProvider.ANILIST -> dao.byAniList(id)
                ListProvider.MAL -> dao.byMal(id)
            }
            val key = existing?.key ?: keyOf(r.ref) ?: continue
            remoteByKey[key] = r
        }
        val localRows = dao.all().associateBy { it.key }
        val keys = LinkedHashSet<String>().apply {
            addAll(localRows.filterValues { it.status != null || it.base(provider) != null }.keys)
            addAll(remoteByKey.keys)
        }
        var pushed = 0
        var pulled = 0
        var conflicts = 0
        var failed = 0
        for (key in keys) {
            val row = localRows[key]
            val remoteEntry = remoteByKey[key]
            val action = decideSync(row?.values(), remoteEntry?.values(), row?.base(provider), key in queued, step, mode)
            try {
                when (action) {
                    SyncAction.NOOP -> Unit
                    SyncAction.ADOPT -> row?.let { dao.upsert(it.withBase(provider, remoteEntry?.values())) }
                    SyncAction.PULL -> {
                        val base = row ?: remoteEntry!!.toRow(key).copy(status = null)
                        dao.upsert(base.applyRemote(remoteEntry, clock()).withBase(provider, remoteEntry?.values()))
                        pulled++
                    }
                    SyncAction.PUSH -> {
                        if (row != null) {
                            pushRow(provider, api, row)
                            pushed++
                        }
                    }
                    SyncAction.CONFLICT -> {
                        if (row != null && remoteEntry != null) {
                            recordConflict(provider, row, remoteEntry)
                            conflicts++
                        }
                    }
                }
                if (key in queued) dao.deleteChange(provider.id, key, OP_SAVE)
            } catch (e: CancellationException) {
                throw e
            } catch (e: AppException) {
                if (e.error is AppError.Offline || e.error is AppError.RateLimited) throw e
                failed++
                noteFailure(provider, key, OP_SAVE, e)
            }
        }
        if (provider == ListProvider.ANILIST) {
            val f = syncFavorites()
            pushed += f.first
            pulled += f.second
        }
        dao.all().filter { it.status == null && !it.favorite && it.bases().isEmpty() }.forEach { dao.delete(it.key) }
        dao.putAccount(account.copy(lastSyncedAtMs = clock()))
        return ListSyncReport(pushed = pushed, pulled = pulled, conflicts = conflicts, failed = failed)
    }

    /** AniList favorites (titles and characters): queued toggles first, then the remote set wins. Returns (pushed, pulled). */
    private suspend fun syncFavorites(): Pair<Int, Int> {
        var pushed = 0
        var pulled = 0
        val nodes = withToken(ListProvider.ANILIST) { anilist.favoriteAnimeNodes(it) }
        val remoteFavs = nodes.map { it.id }.toMutableSet()
        for (change in dao.changes().filter { it.provider == ListProvider.ANILIST.id && it.op == OP_FAVORITE }) {
            val row = dao.get(change.key)
            val id = row?.anilistId
            val wanted = change.favorite ?: row?.favorite ?: false
            if (id != null && (id in remoteFavs) != wanted) {
                withToken(ListProvider.ANILIST) { anilist.toggleFavorite(it, animeId = id) }
                if (wanted) remoteFavs += id else remoteFavs -= id
                pushed++
            }
            dao.deleteChange(change.provider, change.key, change.op)
        }
        for (row in dao.all()) {
            val id = row.anilistId ?: continue
            val fav = id in remoteFavs
            if (row.favorite != fav) dao.upsert(row.copy(favorite = fav))
        }
        for (n in nodes) {
            if (dao.byAniList(n.id) != null) continue
            dao.upsert(
                ListEntryRow(
                    key = n.id.toString(), anilistId = n.id, malId = n.malId, title = n.name, poster = n.image, status = null,
                    totalEpisodes = n.episodes, favorite = true, updatedAtMs = clock(),
                ),
            )
            pulled++
        }
        pushed += pushCharacters()
        val remoteChars = withToken(ListProvider.ANILIST) { anilist.favoriteCharacterNodes(it) }
        val remoteCharIds = remoteChars.map { it.id }.toSet()
        for (c in dao.observeCharacters().first()) {
            if (c.pendingRemote == null && c.id !in remoteCharIds) dao.deleteCharacter(c.id)
        }
        for (n in remoteChars) {
            if (dao.character(n.id) == null) {
                dao.putCharacter(FavoriteCharacterRow(n.id, n.name, n.image, clock()))
                pulled++
            }
        }
        return pushed to pulled
    }

    private suspend fun pushCharacters(): Int {
        var pushed = 0
        for (c in dao.pendingCharacters()) {
            val wanted = c.pendingRemote ?: continue
            val now = withToken(ListProvider.ANILIST) { anilist.isFavouriteCharacter(it, c.id) }
            if (now != wanted) withToken(ListProvider.ANILIST) { anilist.toggleFavorite(it, characterId = c.id) }
            if (wanted) dao.putCharacter(c.copy(pendingRemote = null)) else dao.deleteCharacter(c.id)
            pushed++
        }
        return pushed
    }

    override suspend fun pushPending(): ListSyncReport = syncMutex.withLock {
        var pushed = 0
        var conflicts = 0
        var failed = 0
        val linked = dao.accounts().map { it.provider }.toSet()
        val waiting = _firstSyncPending.value
        for (change in dao.changes()) {
            val provider = ListProvider.entries.firstOrNull { it.id == change.provider }
            if (provider == null || provider.id !in linked) {
                dao.deleteChange(change.provider, change.key, change.op)
                continue
            }
            // first sync not chosen yet: the change stays queued and is applied by that sync (its `queued` rule)
            if (provider in waiting) continue
            try {
                when (change.op) {
                    OP_FAVORITE -> {
                        val row = dao.get(change.key)
                        val id = row?.anilistId
                        if (id != null && provider == ListProvider.ANILIST) {
                            val wanted = change.favorite ?: row?.favorite ?: false
                            val now = withToken(provider) { anilist.isFavouriteAnime(it, id) }
                            if (now != wanted) withToken(provider) { anilist.toggleFavorite(it, animeId = id) }
                            pushed++
                        }
                        dao.deleteChange(change.provider, change.key, change.op)
                    }
                    else -> {
                        val row = dao.get(change.key)
                        if (row == null) {
                            dao.deleteChange(change.provider, change.key, change.op)
                            continue
                        }
                        val api = remote(provider)
                        val id = providerId(provider, row) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
                        val step = scoreStep(provider, dao.accounts().firstOrNull { it.provider == provider.id }?.scoreFormat)
                        val remoteEntry = withToken(provider) { api.fetchOne(it, id) }
                        when (decideSync(row.values(), remoteEntry?.values(), row.base(provider), queued = true, step = step)) {
                            SyncAction.CONFLICT -> {
                                if (remoteEntry != null) recordConflict(provider, row, remoteEntry)
                                conflicts++
                            }
                            SyncAction.ADOPT -> dao.upsert(row.withBase(provider, remoteEntry?.values()))
                            else -> {
                                pushRow(provider, api, row)
                                pushed++
                            }
                        }
                        dao.deleteChange(change.provider, change.key, change.op)
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: AppException) {
                failed++
                noteFailure(provider, change.key, change.op, e)
                if (e.error is AppError.Offline || e.error is AppError.RateLimited) break
            } catch (e: Exception) {
                failed++
                noteFailure(provider, change.key, change.op, e)
            }
        }
        if (ListProvider.ANILIST.id in linked && ListProvider.ANILIST !in waiting) {
            try {
                pushed += pushCharacters()
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                failed++
            }
        }
        ListSyncReport(pushed = pushed, pulled = 0, conflicts = conflicts, failed = failed)
    }

    /** Saves (or deletes, when off the list) [row] on [provider] and records the agreed values. */
    private suspend fun pushRow(provider: ListProvider, api: ListRemote, row: ListEntryRow) {
        val id = providerId(provider, row) ?: throw AppException(AppError.Unknown(ListErrors.NO_ID))
        val entry = row.toEntry()
        withToken(provider) { token -> if (entry == null) api.delete(token, id) else api.save(token, id, entry) }
        dao.upsert((dao.get(row.key) ?: row).withBase(provider, entry?.values()))
    }

    /** The provider's id of a row, resolving the other id through AniList's public API when missing. */
    private suspend fun providerId(provider: ListProvider, row: ListEntryRow): Int? {
        val known = when (provider) {
            ListProvider.ANILIST -> row.anilistId
            ListProvider.MAL -> row.malId
        }
        if (known != null) return known
        val (a, m) = runCatching { anilist.crossId(row.anilistId, row.malId) }.getOrNull() ?: return null
        dao.get(row.key)?.let { dao.upsert(it.copy(anilistId = it.anilistId ?: a, malId = it.malId ?: m)) }
        return if (provider == ListProvider.ANILIST) a else m
    }

    private suspend fun recordConflict(provider: ListProvider, row: ListEntryRow, remoteEntry: ListEntry) {
        val local = row.toEntry() ?: row.toFavoriteEntry()
        val remote = remoteEntry.copy(ref = MediaRef(anilistId = row.anilistId ?: remoteEntry.ref.anilistId, malId = row.malId ?: remoteEntry.ref.malId))
        dao.putConflict(
            ListConflictRow(
                id = "${provider.id}:${row.key}",
                provider = provider.id,
                key = row.key,
                localJson = listJson.encodeToString(ListEntry.serializer(), local),
                remoteJson = listJson.encodeToString(ListEntry.serializer(), remote),
                createdAtMs = clock(),
            ),
        )
    }

    private suspend fun noteFailure(provider: ListProvider, key: String, op: String, e: Exception) {
        val change = dao.changes().firstOrNull { it.provider == provider.id && it.key == key && it.op == op } ?: return
        val reason = (e as? AppException)?.error?.toString() ?: e::class.java.simpleName
        if (change.attempts + 1 >= MAX_ATTEMPTS && (e as? AppException)?.error !is AppError.Offline) {
            // a change the provider keeps refusing (unknown title, invalid values) must not block the queue forever
            dao.deleteChange(change.provider, change.key, change.op)
            AppLog.w(TAG, "dropped ${provider.id} change after ${change.attempts + 1} attempts: $reason")
        } else {
            dao.putChange(change.copy(attempts = change.attempts + 1, lastError = reason.take(120)))
        }
    }

    override fun conflicts(): Flow<List<ListConflict>> = dao.observeConflicts().map { rows ->
        rows.mapNotNull { c ->
            val provider = ListProvider.entries.firstOrNull { it.id == c.provider } ?: return@mapNotNull null
            val local = runCatching { listJson.decodeFromString(ListEntry.serializer(), c.localJson) }.getOrNull() ?: return@mapNotNull null
            val remote = runCatching { listJson.decodeFromString(ListEntry.serializer(), c.remoteJson) }.getOrNull() ?: return@mapNotNull null
            ListConflict(c.id, provider, local, remote)
        }
    }

    override suspend fun resolveConflict(id: String, choice: ConflictChoice) {
        val c = dao.conflict(id) ?: return
        val provider = ListProvider.entries.firstOrNull { it.id == c.provider }
        val remote = runCatching { listJson.decodeFromString(ListEntry.serializer(), c.remoteJson) }.getOrNull()
        dao.deleteConflict(id)
        if (provider == null || remote == null) return
        val row = dao.get(c.key) ?: remote.toRow(c.key).copy(status = null)
        when (choice) {
            ConflictChoice.KEEP_LOCAL -> {
                // the remote values are acknowledged as the base: the queued push then overwrites them without a new conflict
                dao.upsert(row.withBase(provider, remote.values()))
                dao.putChange(ListChangeRow(provider.id, c.key, OP_SAVE, createdAtMs = clock()))
            }
            ConflictChoice.KEEP_REMOTE -> {
                val next = row.applyRemote(remote, clock()).withBase(provider, remote.values())
                dao.upsert(next)
                // the other linked providers follow the new local values
                dao.accounts().filter { it.provider != provider.id }.forEach { a ->
                    dao.putChange(ListChangeRow(a.provider, c.key, OP_SAVE, createdAtMs = clock()))
                }
            }
        }
        scheduler?.requestPush()
    }

    // ------------------------------------------------------------------------------------------------ import / export

    override suspend fun exportJson(): String = exportEntries(dao.all().mapNotNull { it.toEntry() }.sortedByDescending { it.updatedAtMs })

    override suspend fun importJson(json: String): Result<Int> = try {
        val incoming = try {
            parseExport(json)
        } catch (e: Exception) {
            throw AppException(AppError.Unknown(ListErrors.BAD_FILE), e)
        }
        var count = 0
        for (entry in incoming) {
            val key = keyOf(entry.ref) ?: continue
            val existing = dao.get(key)
            if (!shouldImport(existing, entry)) continue
            val row = entry.toRow(key, existing).copy(favorite = entry.favorite || existing?.favorite == true)
            store(row, queue = OP_SAVE)
            count++
        }
        Result.success(count)
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }

    // ------------------------------------------------------------------------------------------------ tokens

    private suspend fun isLinked(provider: ListProvider): Boolean = dao.accounts().any { it.provider == provider.id }

    private fun remote(provider: ListProvider): ListRemote = when (provider) {
        ListProvider.ANILIST -> anilist
        ListProvider.MAL -> mal
    }

    /** A valid access token (MAL refreshes when expired); [AppError.Unknown] `oauth:expired` when the user must link again. */
    private suspend fun token(provider: ListProvider, forceRefresh: Boolean = false): String {
        val raw = secrets.get(SecretKeys.listToken(provider.id)) ?: throw AppException(AppError.Unknown(ListErrors.NOT_LINKED))
        val set = runCatching { listJson.decodeFromString(TokenSet.serializer(), raw) }.getOrNull()
            ?: throw AppException(AppError.Unknown(ListErrors.EXPIRED))
        if (!forceRefresh && !set.isExpired(clock())) return set.access
        if (provider != ListProvider.MAL || set.refresh == null) throw AppException(AppError.Unknown(ListErrors.EXPIRED))
        val clientId = clientIds(provider)?.trim()?.takeIf { it.isNotEmpty() }
            ?: throw AppException(AppError.Unknown("${ListErrors.NO_CLIENT_ID}:${provider.id}"))
        val fresh = mal.refresh(clientId, set.refresh)
        val stored = fresh.copy(refresh = fresh.refresh ?: set.refresh)
        secrets.put(SecretKeys.listToken(provider.id), listJson.encodeToString(TokenSet.serializer(), stored))
        dao.accounts().firstOrNull { it.provider == provider.id }?.let { dao.putAccount(it.copy(expiresAtMs = stored.expiresAtMs)) }
        return stored.access
    }

    /** Runs [block] with a token; a 401 refreshes once (MAL) or reports `oauth:expired`. */
    private suspend fun <T> withToken(provider: ListProvider, block: suspend (String) -> T): T {
        val first = token(provider)
        return try {
            block(first)
        } catch (e: AppException) {
            if ((e.error as? AppError.Http)?.code != 401) throw e
            if (provider != ListProvider.MAL) throw AppException(AppError.Unknown(ListErrors.EXPIRED), e)
            block(token(provider, forceRefresh = true))
        }
    }

    private fun LinkedAccountRow.toModel(): LinkedAccount? {
        val p = ListProvider.entries.firstOrNull { it.id == provider } ?: return null
        return LinkedAccount(p, userId, username, avatar, linkedAtMs, expiresAtMs)
    }

    private operator fun ListSyncReport.plus(o: ListSyncReport) =
        ListSyncReport(pushed + o.pushed, pulled + o.pulled, conflicts + o.conflicts, failed + o.failed)

    private companion object {
        const val TAG = "lists"
        const val KEY_PENDING = "list.oauth.pending"
        const val OP_SAVE = "save"
        const val OP_FAVORITE = "favorite"
        const val MAX_ATTEMPTS = 8
        const val START_SYNC_DELAY_MS = 4_000L
    }
}
