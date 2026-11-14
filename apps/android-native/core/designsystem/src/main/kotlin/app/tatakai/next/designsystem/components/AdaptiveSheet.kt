package app.tatakai.next.designsystem.components

import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.core.view.WindowInsetsControllerCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowCompat
import androidx.core.view.ViewCompat
import androidx.compose.ui.window.DialogWindowProvider
import androidx.compose.ui.platform.LocalView
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.foundation.shape.RoundedCornerShape

/** Width (dp) from which the sheet becomes a side sheet (window size class Medium+). */
const val SIDE_SHEET_MIN_WIDTH_DP = 600

/**
 * Bottom sheet on compact windows, custom side sheet (anchored surface + scrim) on medium/expanded.
 * `ModalSideSheet` does not exist in material3 1.5.0-alpha15 (review E11).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AdaptiveSheet(
    onDismissRequest: () -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    val wide = LocalConfiguration.current.screenWidthDp >= SIDE_SHEET_MIN_WIDTH_DP
    if (!wide) {
        ModalBottomSheet(onDismissRequest = onDismissRequest, modifier = modifier) {
            Column(Modifier.navigationBarsPadding().padding(bottom = 16.dp), content = content)
        }
    } else {
        // BUG-079: over an immersive screen (the player in landscape) the side sheet keeps the status bar hidden too.
        val hostView = LocalView.current
        val immersive = remember(hostView) {
            ViewCompat.getRootWindowInsets(hostView)?.isVisible(WindowInsetsCompat.Type.statusBars()) == false
        }
        Dialog(
            onDismissRequest = onDismissRequest,
            properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false),
        ) {
            if (immersive) {
                val dialogView = LocalView.current
                LaunchedEffect(dialogView) {
                    (dialogView.parent as? DialogWindowProvider)?.window?.let { w ->
                        WindowCompat.getInsetsController(w, dialogView).apply {
                            systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
                            hide(WindowInsetsCompat.Type.systemBars())
                        }
                    }
                }
            }
            Box(
                Modifier
                    .fillMaxSize()
                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = onDismissRequest),
            ) {
                Surface(
                    modifier = modifier
                        .align(Alignment.CenterEnd)
                        .fillMaxHeight()
                        .width(400.dp)
                        // swallow clicks so tapping inside the sheet does not hit the scrim
                        .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null, onClick = {}),
                    // BUG-079: it touches the right edge of the screen, so only the inner corners are rounded.
                    shape = RoundedCornerShape(topStart = 28.dp, bottomStart = 28.dp),
                    color = MaterialTheme.colorScheme.surfaceContainerLow,
                    tonalElevation = 1.dp,
                ) {
                    Column(
                        Modifier
                            .statusBarsPadding()
                            .navigationBarsPadding()
                            .verticalScroll(rememberScrollState())
                            .padding(vertical = 16.dp),
                        content = content,
                    )
                }
            }
        }
    }
}
