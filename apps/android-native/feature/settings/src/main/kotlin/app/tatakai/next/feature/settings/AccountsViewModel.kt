// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.data.InitialSyncMode
import app.tatakai.next.data.ListErrors
import app.tatakai.next.data.ListsSyncControl
import app.tatakai.next.data.PendingOAuthRedirect
import app.tatakai.next.model.AppError
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListConflict
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListSyncReport
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.filterNotNull
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3, worker q; checklist account.identity, account.oauth.errors,
// account.oauth-deep-link, lists.anilist.oauth, lists.mal.oauth, lists.disconnect, lists.offline-queue, lists.conflicts,
// lists.import-export, lists.auto-progress, lists.auto-add, lists.settings.accounts): Settings › Listas y cuentas.

/** Why a provider cannot be linked / what went wrong, as a stable code the screen maps to a text ([oauthErrorRes]). */
enum class AccountsError { NO_CLIENT_ID, CANCELLED, INVALID_STATE, EXPIRED, EXCHANGE, DENIED, OFFLINE, RATE_LIMITED, HTTP, BAD_FILE, NOT_LINKED, UNKNOWN }

/** Maps a lists / OAuth failure (see `ListErrors` in :core:data) to [AccountsError]. Pure, unit tested. */
fun AppError.toAccountsError(): AccountsError = when (this) {
    AppError.Offline -> AccountsError.OFFLINE
    is AppError.RateLimited -> AccountsError.RATE_LIMITED
    is AppError.Http -> if (code == 401) AccountsError.EXPIRED else AccountsError.HTTP
    is AppError.Extension -> AccountsError.UNKNOWN
    is AppError.Unknown -> when {
        msg.startsWith(ListErrors.NO_CLIENT_ID) -> AccountsError.NO_CLIENT_ID
        msg == ListErrors.CANCELLED -> AccountsError.CANCELLED
        msg == ListErrors.INVALID_STATE -> AccountsError.INVALID_STATE
        msg == ListErrors.EXPIRED -> AccountsError.EXPIRED
        msg == ListErrors.EXCHANGE -> AccountsError.EXCHANGE
        msg.startsWith(ListErrors.DENIED) -> AccountsError.DENIED
        msg == ListErrors.BAD_FILE -> AccountsError.BAD_FILE
        msg == ListErrors.NOT_LINKED -> AccountsError.NOT_LINKED
        else -> AccountsError.UNKNOWN
    }
}

data class AccountsUiState(
    val accounts: List<LinkedAccount> = emptyList(),
    /** Providers with a client id configured (`TATAKAI_ANILIST_CLIENT_ID` / `TATAKAI_MAL_CLIENT_ID`); the others show why "Vincular" is off. */
    val configured: Set<ListProvider> = emptySet(),
    val pendingChanges: Int = 0,
    val lastSyncedAt: Long? = null,
    val conflicts: List<ListConflict> = emptyList(),
    val syncing: Boolean = false,
    /** Provider whose OAuth is running (Custom Tab open or code exchange in flight). */
    val linking: ListProvider? = null,
    /** Last link failure, shown inline on the provider card until the next attempt. */
    val linkError: Pair<ListProvider?, AccountsError>? = null,
    val lastReport: ListSyncReport? = null,
    val transferring: Boolean = false,
    // lists.import-export (parity wave A closure, worker q)
    /** Linked providers whose first sync waits for import remote / export local / merge (the card shows "Elegir"). */
    val firstSyncPending: Set<ListProvider> = emptySet(),
    /** Provider whose sync-mode sheet is open: right after a link with a local list, or reopened from the card. */
    val syncModeSheet: ListProvider? = null,
)

sealed interface AccountsEvent {
    /** Open this authorization URL in a Custom Tab. */
    data class OpenUrl(val url: String) : AccountsEvent
    data class Linked(val account: LinkedAccount) : AccountsEvent
    data class Unlinked(val provider: ListProvider) : AccountsEvent
    data class Synced(val report: ListSyncReport) : AccountsEvent
    data class Exported(val json: String) : AccountsEvent
    data class Imported(val count: Int) : AccountsEvent
    data class Failed(val error: AccountsError) : AccountsEvent
}

class AccountsViewModel(
    private val lists: ListsRepository,
    private val settingsRepo: SettingsRepository,
    private val redirects: PendingOAuthRedirect? = null,
    configured: Set<ListProvider> = ListProvider.entries.toSet(),
) : ViewModel() {

    private data class Local(
        val syncing: Boolean = false,
        val linking: ListProvider? = null,
        val linkError: Pair<ListProvider?, AccountsError>? = null,
        val lastReport: ListSyncReport? = null,
        val transferring: Boolean = false,
        val exchanging: Boolean = false,
        /** Sheet reopened from the card (not a pending first sync). */
        val asking: ListProvider? = null,
        /** Pending first syncs whose sheet the user closed this time (the card keeps the "Elegir" button). */
        val dismissed: Set<ListProvider> = emptySet(),
    )

    private val local = MutableStateFlow(Local())
    private val _events = Channel<AccountsEvent>(Channel.BUFFERED)
    val events: Flow<AccountsEvent> = _events.receiveAsFlow()

    private val control = lists as? ListsSyncControl

    val state: StateFlow<AccountsUiState> = combine(
        lists.accounts,
        lists.pendingChanges,
        lists.lastSyncedAt,
        lists.conflicts().catch { emit(emptyList()) },
        combine(local, control?.firstSyncPending ?: flowOf(emptySet<ListProvider>())) { l, p -> l to p },
    ) { accounts, pending, synced, conflicts, (l, firstPending) ->
        AccountsUiState(
            accounts = accounts,
            configured = configured,
            pendingChanges = pending,
            lastSyncedAt = synced,
            conflicts = conflicts,
            syncing = l.syncing,
            linking = l.linking,
            linkError = l.linkError,
            lastReport = l.lastReport,
            transferring = l.transferring,
            firstSyncPending = firstPending,
            syncModeSheet = syncModeSheetOf(l.asking, firstPending, l.dismissed, l.syncing),
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), AccountsUiState(configured = configured))

    init {
        // tatakai://oauth/<provider>?code=…&state=… handed over by MainActivity (cold start or onNewIntent)
        redirects?.let { pending ->
            viewModelScope.launch {
                pending.link.filterNotNull().collect {
                    val uri = pending.consume() ?: return@collect
                    complete(uri)
                }
            }
        }
    }

    fun link(provider: ListProvider) {
        if (local.value.linking != null) return
        local.update { it.copy(linking = provider, linkError = null) }
        viewModelScope.launch {
            lists.beginLink(provider).fold(
                onSuccess = { url -> _events.trySend(AccountsEvent.OpenUrl(url)) },
                onFailure = { e ->
                    val error = e.toAppError().toAccountsError()
                    local.update { it.copy(linking = null, linkError = provider to error) }
                },
            )
        }
    }

    /** The Custom Tab came back without a redirect (user closed it): stop the spinner, keep the pending state for a late redirect. */
    fun linkTabClosed() {
        local.update { if (it.linking != null && !it.exchanging) it.copy(linking = null) else it }
    }

    internal fun complete(redirectUri: String) {
        local.update { it.copy(linking = providerOf(redirectUri) ?: it.linking, linkError = null, exchanging = true) }
        viewModelScope.launch {
            val result = runCatchingApp { lists.completeLink(redirectUri).getOrThrow() }
            result.fold(
                onSuccess = { account ->
                    local.update { it.copy(linking = null, linkError = null, exchanging = false) }
                    _events.trySend(AccountsEvent.Linked(account))
                },
                onFailure = { e ->
                    val error = e.toAppError().toAccountsError()
                    local.update { it.copy(linking = null, linkError = providerOf(redirectUri) to error, exchanging = false) }
                    _events.trySend(AccountsEvent.Failed(error))
                },
            )
        }
    }

    fun unlink(provider: ListProvider) {
        viewModelScope.launch {
            runCatchingApp { lists.unlink(provider) }.fold(
                onSuccess = { _events.trySend(AccountsEvent.Unlinked(provider)) },
                onFailure = { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) },
            )
        }
    }

    fun syncNow() {
        if (local.value.syncing) return
        local.update { it.copy(syncing = true) }
        viewModelScope.launch {
            val result = runCatchingApp { lists.syncNow().getOrThrow() }
            local.update { it.copy(syncing = false, lastReport = result.getOrNull() ?: it.lastReport) }
            result.fold(
                onSuccess = { _events.trySend(AccountsEvent.Synced(it)) },
                onFailure = { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) },
            )
        }
    }

    // ---------------------------------------------------------------- first sync mode (lists.import-export)

    /** Opens the import / export / merge sheet for a linked [provider] (card action; also how a closed first-sync sheet comes back). */
    fun askSyncMode(provider: ListProvider) {
        if (lists.accounts.value.none { it.provider == provider }) return
        local.update { it.copy(asking = provider, dismissed = it.dismissed - provider) }
    }

    /** Closes the sheet; a pending first sync stays pending (no merge behind the user's back) and the card offers it again. */
    fun dismissSyncMode() {
        val l = local.value
        val open = syncModeSheetOf(l.asking, control?.firstSyncPending?.value.orEmpty(), l.dismissed, l.syncing)
        local.update { it.copy(asking = null, dismissed = if (open != null) it.dismissed + open else it.dismissed) }
    }

    /** Runs the sync of [provider] in [mode]: IMPORT = remote wins, EXPORT = local wins, MERGE = disagreements become conflicts. */
    fun syncWithMode(provider: ListProvider, mode: InitialSyncMode) {
        val c = control ?: return
        if (local.value.syncing) return
        local.update { it.copy(syncing = true, asking = null, dismissed = it.dismissed - provider) }
        viewModelScope.launch {
            val result = runCatchingApp { c.syncWith(provider, mode).getOrThrow() }
            local.update { it.copy(syncing = false, lastReport = result.getOrNull() ?: it.lastReport) }
            result.fold(
                onSuccess = { _events.trySend(AccountsEvent.Synced(it)) },
                onFailure = { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) },
            )
        }
    }

    fun resolve(conflict: ListConflict, choice: ConflictChoice) {
        viewModelScope.launch {
            runCatchingApp { lists.resolveConflict(conflict.id, choice) }
                .onFailure { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) }
        }
    }

    fun resolveAll(choice: ConflictChoice) {
        val all = state.value.conflicts
        viewModelScope.launch {
            all.forEach { c -> runCatchingApp { lists.resolveConflict(c.id, choice) } }
        }
    }

    fun setAutoProgress(on: Boolean) = viewModelScope.launch { settingsRepo.update { it.copy(autoListProgress = on) } }

    fun setAutoAdd(on: Boolean) = viewModelScope.launch { settingsRepo.update { it.copy(autoListAdd = on) } }

    /** Builds the export document; the screen writes it to the SAF file the user picked. */
    fun export() {
        if (local.value.transferring) return
        local.update { it.copy(transferring = true) }
        viewModelScope.launch {
            val result = runCatchingApp { lists.exportJson() }
            local.update { it.copy(transferring = false) }
            result.fold(
                onSuccess = { _events.trySend(AccountsEvent.Exported(it)) },
                onFailure = { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) },
            )
        }
    }

    /** [json] = content of the file picked with SAF (null = it could not be read). */
    fun import(json: String?) {
        if (json == null) {
            _events.trySend(AccountsEvent.Failed(AccountsError.BAD_FILE))
            return
        }
        local.update { it.copy(transferring = true) }
        viewModelScope.launch {
            val result = runCatchingApp { lists.importJson(json).getOrThrow() }
            local.update { it.copy(transferring = false) }
            result.fold(
                onSuccess = { _events.trySend(AccountsEvent.Imported(it)) },
                onFailure = { _events.trySend(AccountsEvent.Failed(it.toAppError().toAccountsError())) },
            )
        }
    }

    private fun providerOf(uri: String): ListProvider? = app.tatakai.next.data.OAuthRedirect.providerOf(uri)
}

/**
 * Which provider's sync-mode sheet is open: the one reopened from the card first, else the first pending first sync the user did not
 * close; none while a sync runs. Pure, unit tested.
 */
internal fun syncModeSheetOf(
    asking: ListProvider?,
    firstSyncPending: Set<ListProvider>,
    dismissed: Set<ListProvider>,
    syncing: Boolean,
): ListProvider? {
    if (syncing) return null
    if (asking != null) return asking
    return ListProvider.entries.firstOrNull { it in firstSyncPending && it !in dismissed }
}
