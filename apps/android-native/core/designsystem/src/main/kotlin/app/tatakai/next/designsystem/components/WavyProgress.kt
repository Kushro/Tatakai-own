// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * M3E wavy progress (`LinearWavyProgressIndicator` / `CircularWavyProgressIndicator`, confirmed in material3 1.5.0-alpha15).
 * [progress] 0..1, null = indeterminate. The wave phase does not animate when `LocalReduceMotion` is true. [thin] = 4 dp track.
 * Body: worker b.
 */
@Composable
fun WavyProgress(
    progress: Float?,
    modifier: Modifier = Modifier,
    circular: Boolean = false,
    thin: Boolean = false,
) {
    Text("WavyProgress", modifier)
}
