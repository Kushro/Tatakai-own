// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.Snackbar
import androidx.compose.material3.SnackbarDuration
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.
// ONE global snackbar host in the app shell; screens show messages through LocalAppSnackbar.current.show(...).

interface AppSnackbar {
    /** Short message (already translated), optional action; auto-dismiss (with action: long duration). */
    fun show(message: String, actionLabel: String? = null, onAction: (() -> Unit)? = null)

    companion object {
        /** Default outside the app shell (previews, tests): drops the message. */
        val None: AppSnackbar = object : AppSnackbar {
            override fun show(message: String, actionLabel: String?, onAction: (() -> Unit)?) = Unit
        }
    }
}

val LocalAppSnackbar: ProvidableCompositionLocal<AppSnackbar> = staticCompositionLocalOf { AppSnackbar.None }

/**
 * The shell's implementation: queues messages into [hostState]. `SnackbarHostState.showSnackbar` already serialises calls (one
 * visible at a time, FIFO); a message identical to the one on screen is dropped so repeated taps do not stack the queue.
 */
@Stable
class AppSnackbarController(val hostState: SnackbarHostState, private val scope: CoroutineScope) : AppSnackbar {
    override fun show(message: String, actionLabel: String?, onAction: (() -> Unit)?) {
        if (message.isBlank()) return
        val visible = hostState.currentSnackbarData?.visuals
        if (visible != null && visible.message == message && visible.actionLabel == actionLabel) return
        scope.launch {
            val result = hostState.showSnackbar(
                message = message,
                actionLabel = actionLabel,
                withDismissAction = false,
                duration = if (actionLabel != null) SnackbarDuration.Long else SnackbarDuration.Short,
            )
            if (result == SnackbarResult.ActionPerformed) onAction?.invoke()
        }
    }
}

@Composable
fun rememberAppSnackbarController(): AppSnackbarController {
    val scope = rememberCoroutineScope()
    return remember(scope) { AppSnackbarController(SnackbarHostState(), scope) }
}

/** Placed once by the app shell (above the navigation bar, below the player). */
@Composable
fun AppSnackbarHost(controller: AppSnackbarController, modifier: Modifier = Modifier) {
    SnackbarHost(controller.hostState, modifier) { data -> Snackbar(data) }
}
