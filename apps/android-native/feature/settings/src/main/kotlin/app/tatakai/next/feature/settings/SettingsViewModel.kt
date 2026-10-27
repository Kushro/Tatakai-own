package app.tatakai.next.feature.settings

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AccountSuspendedException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.UserSession
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class SignInError { INVALID, SUSPENDED }

data class SettingsUiState(
    val settings: AppSettings = AppSettings(),
    val session: UserSession? = null,
    val signingIn: Boolean = false,
    val signInError: SignInError? = null,
)

class SettingsViewModel(
    private val settingsRepo: SettingsRepository,
    private val auth: AuthRepository,
) : ViewModel() {

    private val signInState = MutableStateFlow(SignInProgress())

    val state: StateFlow<SettingsUiState> = combine(settingsRepo.settings, auth.session, signInState) { s, session, progress ->
        SettingsUiState(settings = s, session = session, signingIn = progress.working, signInError = progress.error)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), SettingsUiState(settings = settingsRepo.settings.value, session = auth.session.value))

    fun setPalette(id: String) = update { it.copy(paletteId = id) }

    fun setAmoled(value: Boolean) = update { it.copy(amoled = value) }

    fun setDynamicColor(value: Boolean) = update { it.copy(dynamicColor = value) }

    fun setContrast(value: ContrastPref) = update { it.copy(contrast = value) }

    /** Moves [providerId] by [delta] positions in the metadata priority list (negative = earlier). */
    fun moveProvider(providerId: String, delta: Int) = update { it.copy(providerPriority = it.providerPriority.moved(providerId, delta)) }

    fun signIn(email: String, password: String) {
        if (signInState.value.working) return
        signInState.value = SignInProgress(working = true)
        viewModelScope.launch {
            val result = auth.signIn(email.trim(), password)
            signInState.value = result.fold(
                onSuccess = { SignInProgress() },
                onFailure = { e ->
                    SignInProgress(error = if (e is AccountSuspendedException) SignInError.SUSPENDED else SignInError.INVALID)
                },
            )
        }
    }

    fun dismissSignInError() = signInState.update { it.copy(error = null) }

    fun signOut() {
        viewModelScope.launch { auth.signOut() }
    }

    private fun update(transform: (AppSettings) -> AppSettings) {
        viewModelScope.launch { settingsRepo.update(transform) }
    }

    private data class SignInProgress(val working: Boolean = false, val error: SignInError? = null)
}

/** Pure list move used by the provider priority editor; ignores unknown ids and clamps to the list bounds. */
fun List<String>.moved(id: String, delta: Int): List<String> {
    val from = indexOf(id)
    if (from < 0) return this
    val to = (from + delta).coerceIn(0, lastIndex)
    if (to == from) return this
    return toMutableList().also {
        it.removeAt(from)
        it.add(to, id)
    }
}
