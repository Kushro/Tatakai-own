// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionPreference
import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.ExtensionUpdate
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.StremioAddon
import app.tatakai.next.model.StremioAddonRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t): the hub sections of Extensions - updates (badge +
// sheet), install from URL, repositories by URL, native Stremio addons, custom providers, per-extension preferences and the kill switch
// ("Desactivar todas" / "Activar todas"). Classic: ExtensionHubPage, ExtensionUpdateSheet, RemoteExtensionsPanel, CustomProvidersPage.

/** One-shot outcome shown in the snackbar. [arg] is a name / version / reason already in the user's language or technical. */
sealed interface HubNotice {
    data class Updated(val name: String, val version: String) : HubNotice
    data class UpdatesChecked(val count: Int) : HubNotice
    data class RepoAdded(val name: String) : HubNotice
    data class AddonInstalled(val name: String) : HubNotice
    data class AddonUpdated(val name: String, val version: String) : HubNotice
    data object MatchesReset : HubNotice
    data class AllEnabled(val enabled: Boolean) : HubNotice
    data class Failed(val reason: String) : HubNotice
}

data class HubUiState(
    val updates: List<ExtensionUpdate> = emptyList(),
    /** Extension ids being updated. */
    val updating: Set<String> = emptySet(),
    val checkingUpdates: Boolean = false,
    /** `AppSettings.extensionUpdateCheck` (check the repositories at app start). */
    val updateCheckOnStart: Boolean = true,
    val repos: List<ExtensionRepo> = emptyList(),
    /** Repository URLs being added / refreshed, and `.kai` URLs being installed from a repository. */
    val busy: Set<String> = emptySet(),
    val addRepoError: String? = null,
    val addons: List<StremioAddon> = emptyList(),
    val stremioAvailable: Boolean = false,
    val addonError: String? = null,
    val providers: LoadState<List<CustomSourceRef>> = LoadState.Loading,
    /** Installed extensions whose manifest declares `preferences[]` (the card shows "Preferencias"). */
    val withPreferences: Set<String> = emptySet(),
)

@OptIn(FlowPreview::class)
class ExtensionsHubViewModel(
    private val repo: ExtensionRepository,
    private val stremio: StremioAddonRepository? = null,
    private val settings: SettingsRepository? = null,
) : ViewModel() {
    private val local = MutableStateFlow(Local())
    private val providers = MutableStateFlow<LoadState<List<CustomSourceRef>>>(LoadState.Loading)
    private val withPrefs = MutableStateFlow<Set<String>>(emptySet())
    private val _notices = Channel<HubNotice>(Channel.BUFFERED)
    val notices: Flow<HubNotice> = _notices.receiveAsFlow()

    private val addonsFlow: Flow<List<StremioAddon>> = stremio?.addons ?: flowOf(emptyList())
    private val updateCheckFlow: Flow<Boolean> = settings?.settings?.map { it.extensionUpdateCheck }?.distinctUntilChanged() ?: flowOf(true)

    private val base = combine(repo.updates, repo.repositories, addonsFlow, updateCheckFlow) { u, r, a, check -> Base(u, r, a, check) }

    val state: StateFlow<HubUiState> = combine(base, local, providers, withPrefs) { b, l, p, w ->
        HubUiState(
            updates = b.updates,
            updating = l.updating,
            checkingUpdates = l.checking,
            updateCheckOnStart = b.updateCheck,
            repos = b.repos,
            busy = l.busy,
            addRepoError = l.addRepoError,
            addons = b.addons,
            stremioAvailable = stremio != null,
            addonError = l.addonError,
            providers = p,
            withPreferences = w,
        )
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), HubUiState(stremioAvailable = stremio != null))

    init {
        viewModelScope.launch {
            // Enabling / installing / removing an extension changes its custom sources and preference forms.
            repo.installed.map { list -> list.map { it.id to it.enabled } }.distinctUntilChanged().debounce(RELOAD_DEBOUNCE_MS).collect {
                loadProviders()
                loadPreferenceOwners()
            }
        }
    }

    // ---- providers (ext.custom-providers.list) ----------------------------------------------------------------------------------

    fun loadProviders() {
        viewModelScope.launch {
            if (providers.value !is LoadState.Ok) providers.value = LoadState.Loading
            providers.value = try {
                LoadState.Ok(repo.customSources(null))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toError())
            }
        }
    }

    private suspend fun loadPreferenceOwners() {
        val ids = repo.installed.value.map { it.id }
        val owners = ids.filter { id -> runCatchingNonCancel { repo.preferences(id).isNotEmpty() } == true }.toSet()
        withPrefs.value = owners
    }

    // ---- preferences (ext.preferences) ------------------------------------------------------------------------------------------

    suspend fun preferences(extensionId: String): List<ExtensionPreference> = repo.preferences(extensionId)

    fun preferenceValues(extensionId: String): Flow<Map<String, String>> = repo.preferenceValues(extensionId)

    fun setPreference(extensionId: String, key: String, value: String?) {
        viewModelScope.launch { runCatchingNonCancel { repo.setPreference(extensionId, key, value) } }
    }

    // ---- updates (ext.updates.check / ext.updates.sheet) -----------------------------------------------------------------------

    fun checkUpdates() {
        if (local.value.checking) return
        local.update { it.copy(checking = true) }
        viewModelScope.launch {
            try {
                repo.repositories.value.forEach { r -> runCatchingNonCancel { repo.refreshRepository(r.url) } }
                val found = repo.checkUpdates()
                _notices.trySend(HubNotice.UpdatesChecked(found.size))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _notices.trySend(HubNotice.Failed(e.reason()))
            } finally {
                local.update { it.copy(checking = false) }
            }
        }
    }

    fun update(extensionId: String) {
        if (extensionId in local.value.updating) return
        local.update { it.copy(updating = it.updating + extensionId) }
        viewModelScope.launch {
            try {
                repo.update(extensionId).fold(
                    onSuccess = { _notices.trySend(HubNotice.Updated(it.name, it.version)) },
                    onFailure = { _notices.trySend(HubNotice.Failed(it.reason())) },
                )
            } finally {
                local.update { it.copy(updating = it.updating - extensionId) }
            }
        }
    }

    /** "Actualizar todas": one after the other (the host installs serially anyway). */
    fun updateAll() {
        val pending = state.value.updates.map { it.id }.filterNot { it in local.value.updating }
        if (pending.isEmpty()) return
        local.update { it.copy(updating = it.updating + pending) }
        viewModelScope.launch {
            for (id in pending) {
                try {
                    repo.update(id).fold(
                        onSuccess = { _notices.trySend(HubNotice.Updated(it.name, it.version)) },
                        onFailure = { _notices.trySend(HubNotice.Failed(it.reason())) },
                    )
                } finally {
                    local.update { it.copy(updating = it.updating - id) }
                }
            }
        }
    }

    fun setUpdateCheckOnStart(on: Boolean) {
        val s = settings ?: return
        viewModelScope.launch { runCatchingNonCancel { s.update { it.copy(extensionUpdateCheck = on) } } }
    }

    // ---- repositories (ext.repositories) ----------------------------------------------------------------------------------------

    fun addRepository(url: String) {
        val clean = url.trim()
        if (clean.isEmpty() || clean in local.value.busy) return
        local.update { it.copy(busy = it.busy + clean, addRepoError = null) }
        viewModelScope.launch {
            try {
                repo.addRepository(clean).fold(
                    onSuccess = { r ->
                        _notices.trySend(HubNotice.RepoAdded(r.name))
                        runCatchingNonCancel { repo.checkUpdates() }
                    },
                    onFailure = { e -> local.update { it.copy(addRepoError = e.reason()) } },
                )
            } finally {
                local.update { it.copy(busy = it.busy - clean) }
            }
        }
    }

    fun refreshRepository(url: String) = busyCall(url) {
        repo.refreshRepository(url).onFailure { e -> _notices.trySend(HubNotice.Failed(e.reason())) }
        runCatchingNonCancel { repo.checkUpdates() }
    }

    fun removeRepository(url: String) = busyCall(url) { repo.removeRepository(url) }

    /** Install (or reinstall) one `.kai` listed by a repository. */
    fun installEntry(packageUrl: String) = busyCall(packageUrl) {
        repo.installFromUrl(packageUrl).fold(
            onSuccess = { _notices.trySend(HubNotice.Updated(it.name, it.version)) },
            onFailure = { e -> _notices.trySend(HubNotice.Failed(e.reason())) },
        )
        runCatchingNonCancel { repo.checkUpdates() }
    }

    fun clearRepoError() = local.update { it.copy(addRepoError = null) }

    // ---- Stremio (ext.stremio / ext.stremio.manage) -----------------------------------------------------------------------------

    fun installAddon(manifestUrl: String) {
        val s = stremio ?: return
        val clean = manifestUrl.trim()
        if (clean.isEmpty() || clean in local.value.busy) return
        local.update { it.copy(busy = it.busy + clean, addonError = null) }
        viewModelScope.launch {
            try {
                s.install(clean).fold(
                    onSuccess = { _notices.trySend(HubNotice.AddonInstalled(it.name)) },
                    onFailure = { e -> local.update { it.copy(addonError = e.reason()) } },
                )
            } finally {
                local.update { it.copy(busy = it.busy - clean) }
            }
        }
    }

    fun setAddonEnabled(id: String, enabled: Boolean) {
        val s = stremio ?: return
        viewModelScope.launch { runCatchingNonCancel { s.setEnabled(id, enabled) } }
    }

    fun updateAddon(id: String) {
        val s = stremio ?: return
        busyCall(ADDON_PREFIX + id) {
            s.update(id).fold(
                onSuccess = { _notices.trySend(HubNotice.AddonUpdated(it.name, it.version)) },
                onFailure = { e -> _notices.trySend(HubNotice.Failed(e.reason())) },
            )
        }
    }

    fun resetAddonMatches(id: String) {
        val s = stremio ?: return
        viewModelScope.launch {
            runCatchingNonCancel { s.resetMatches(id) }
            _notices.trySend(HubNotice.MatchesReset)
        }
    }

    fun removeAddon(id: String) {
        val s = stremio ?: return
        viewModelScope.launch { runCatchingNonCancel { s.remove(id) } }
    }

    fun clearAddonError() = local.update { it.copy(addonError = null) }

    // ---- kill switch (ext.kill-switch) ------------------------------------------------------------------------------------------

    fun setAllEnabled(enabled: Boolean) {
        viewModelScope.launch {
            try {
                repo.setAllEnabled(enabled)
                _notices.trySend(HubNotice.AllEnabled(enabled))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _notices.trySend(HubNotice.Failed(e.reason()))
            }
        }
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private fun busyCall(key: String, block: suspend () -> Unit) {
        if (key in local.value.busy) return
        local.update { it.copy(busy = it.busy + key) }
        viewModelScope.launch {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _notices.trySend(HubNotice.Failed(e.reason()))
            } finally {
                local.update { it.copy(busy = it.busy - key) }
            }
        }
    }

    private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }

    private data class Base(val updates: List<ExtensionUpdate>, val repos: List<ExtensionRepo>, val addons: List<StremioAddon>, val updateCheck: Boolean)

    private data class Local(
        val updating: Set<String> = emptySet(),
        val checking: Boolean = false,
        val busy: Set<String> = emptySet(),
        val addRepoError: String? = null,
        val addonError: String? = null,
    )

    companion object {
        const val RELOAD_DEBOUNCE_MS = 300L

        /** Busy key of a Stremio addon action (kept apart from URLs). */
        const val ADDON_PREFIX = "addon:"
    }
}

/** Technical reason of a failure (the host's validation message, the HTTP error...), never empty. */
internal fun Throwable.reason(): String = when (this) {
    is AppException -> when (val e = error) {
        is AppError.Extension -> e.msg
        is AppError.Unknown -> e.msg
        is AppError.Http -> "HTTP ${e.code}"
        is AppError.RateLimited -> e.provider
        AppError.Offline -> message ?: "offline"
    }
    else -> message?.takeIf { it.isNotBlank() } ?: this::class.java.simpleName
}

internal fun Throwable.toError(): AppError = when (this) {
    is AppException -> error
    is java.io.IOException -> AppError.Offline
    else -> AppError.Unknown(message?.takeIf { it.isNotBlank() } ?: this::class.java.simpleName)
}
