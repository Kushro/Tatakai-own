// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

data class SegmentedOption<T>(
    val value: T,
    val label: String,
    val enabled: Boolean = true,
    val icon: ImageVector? = null,
    /** Accessibility / tooltip text (e.g. a preset hint). */
    val description: String? = null,
)

/**
 * Connected single-choice button group (prototype `.seg`, desktop `Segmented`): M3E `ButtonGroup` + `ToggleButton` or
 * `SingleChoiceSegmentedButtonRow` (both confirmed in .deps-sources material3 1.5.0-alpha15). [small] = compact height. Body: worker b.
 */
@Composable
fun <T> Segmented(
    options: List<SegmentedOption<T>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    small: Boolean = false,
) {
    Text("Segmented", modifier)
}
