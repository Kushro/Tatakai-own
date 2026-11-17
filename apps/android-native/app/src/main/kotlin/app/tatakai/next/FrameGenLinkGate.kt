package app.tatakai.next

import app.tatakai.next.framegen.FgController
import app.tatakai.next.framegen.FgUnavailableReason
import app.tatakai.next.model.PhoneFrameGenAvailability
import app.tatakai.next.model.PhoneFrameGenGate
import app.tatakai.next.model.ProcessingLocation
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn

/**
 * 0.4.0 integration (Link modes x phone frame generation). "Mixto" can be chosen while the phone could generate frames: only
 * ineligibility (ABI, library, Vulkan, shaders not imported) or a failure blocks it; temporary pauses (PiP, background, the
 * PC session itself) do not. Locations: PHONE = the user's own FG setting; PC = FG off (the PC already processes
 * everything); MIXED = FG on over the PC's stream. If Mixto had to turn FG on, leaving Mixto turns it off again, so the
 * user's saved preference is what it was.
 */
class FrameGenLinkGate(private val fg: FgController, scope: CoroutineScope) : PhoneFrameGenGate {
    override val availability: StateFlow<PhoneFrameGenAvailability> = fg.state
        .map { s ->
            val reason = s.unavailable
            when {
                reason == null || reason.kind == FgUnavailableReason.Kind.SUSPENDED -> PhoneFrameGenAvailability(eligible = true)
                else -> PhoneFrameGenAvailability(eligible = false, reason = reason.name.lowercase())
            }
        }
        .stateIn(scope, SharingStarted.Eagerly, PhoneFrameGenAvailability(eligible = false, reason = "checking"))

    private var enabledByMixed = false

    init {
        fg.refresh()
    }

    override fun setProcessingLocation(location: ProcessingLocation) {
        when (location) {
            ProcessingLocation.PHONE -> {
                fg.onRemote(active = false)
                restoreUserChoice()
            }
            ProcessingLocation.PC -> {
                fg.onRemote(active = true, phoneFgAllowed = false)
                restoreUserChoice()
            }
            ProcessingLocation.MIXED -> {
                fg.onRemote(active = true, phoneFgAllowed = true)
                if (!fg.state.value.enabled) {
                    fg.setEnabled(true)
                    enabledByMixed = fg.state.value.enabled
                }
            }
        }
    }

    private fun restoreUserChoice() {
        if (enabledByMixed) {
            enabledByMixed = false
            fg.setEnabled(false)
        }
    }
}
