package app.tatakai.next.designsystem.components

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import kotlinx.coroutines.delay

/** Placeholder block with the final shape of the content (02 §5). */
@Composable
fun SkeletonBox(modifier: Modifier = Modifier, shape: Shape = MaterialTheme.shapes.medium) {
    val transition = rememberInfiniteTransition(label = "skeleton")
    val alpha by transition.animateFloat(
        initialValue = 0.45f,
        targetValue = 0.9f,
        animationSpec = infiniteRepeatable(tween(900), RepeatMode.Reverse),
        label = "skeletonAlpha",
    )
    Box(
        modifier
            .alpha(alpha)
            .clip(shape)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
    )
}

/** `true` once [delayMillis] elapsed; used to show skeletons only after 150 ms (02 §5). */
@Composable
fun rememberAfterDelay(delayMillis: Long = 150): Boolean {
    var ready by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        delay(delayMillis)
        ready = true
    }
    return ready
}
