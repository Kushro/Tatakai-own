package app.tatakai.next.player

import app.tatakai.next.model.LocalEnhancementGate
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * 0.4.0 integration (Link modes x quality engine): whether the PC processes the stream that is playing ("PC" and "Mixto").
 * The player ViewModel sets it through [LocalEnhancementGate]; every libmpv session follows it, including one created right
 * after a switch, so the phone never runs its own shaders on a stream the PC already processed.
 */
class RemoteProcessingState : LocalEnhancementGate {
    private val _active = MutableStateFlow(false)
    val active: StateFlow<Boolean> = _active.asStateFlow()

    override fun setRemoteProcessing(on: Boolean) {
        _active.value = on
    }
}
