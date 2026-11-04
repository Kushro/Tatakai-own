// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import kotlinx.coroutines.CoroutineScope

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

/** The shell's implementation: queues messages into [hostState]. Body of [show]: worker b. */
@Stable
class AppSnackbarController(val hostState: SnackbarHostState, private val scope: CoroutineScope) : AppSnackbar {
    override fun show(message: String, actionLabel: String?, onAction: (() -> Unit)?) {
        // worker b: scope.launch { hostState.showSnackbar(...) ; run onAction on ActionPerformed }
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
    SnackbarHost(controller.hostState, modifier)
}
