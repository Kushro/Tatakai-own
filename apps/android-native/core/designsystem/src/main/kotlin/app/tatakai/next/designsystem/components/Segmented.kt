// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

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
 * `SingleChoiceSegmentedButtonRow` (both confirmed in .deps-sources material3 1.5.0-alpha15). [small] = compact height.
 *
 * Implementation: `SingleChoiceSegmentedButtonRow` + `SegmentedButton` (connected outline, check icon on the selected item);
 * the row scrolls horizontally when the options do not fit (6 presets on a phone).
 */
@Composable
fun <T> Segmented(
    options: List<SegmentedOption<T>>,
    selected: T,
    onSelect: (T) -> Unit,
    modifier: Modifier = Modifier,
    small: Boolean = false,
) {
    Row(modifier.horizontalScroll(rememberScrollState())) {
        SingleChoiceSegmentedButtonRow {
            options.forEachIndexed { index, option ->
                val isSelected = option.value == selected
                SegmentedButton(
                    selected = isSelected,
                    onClick = { if (!isSelected) onSelect(option.value) },
                    shape = SegmentedButtonDefaults.itemShape(index = index, count = options.size),
                    enabled = option.enabled,
                    contentPadding = if (small) SMALL_PADDING else SegmentedButtonDefaults.ContentPadding,
                    modifier = if (option.description != null) {
                        Modifier.semantics { contentDescription = "${option.label}. ${option.description}" }
                    } else {
                        Modifier
                    },
                    icon = {
                        val icon = option.icon
                        if (icon != null && !isSelected) {
                            Icon(icon, contentDescription = null, modifier = Modifier.size(SegmentedButtonDefaults.IconSize))
                        } else {
                            SegmentedButtonDefaults.Icon(isSelected)
                        }
                    },
                    label = {
                        Text(
                            option.label,
                            style = if (small) MaterialTheme.typography.labelMedium else MaterialTheme.typography.labelLarge,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    },
                )
            }
        }
    }
}

private val SMALL_PADDING = PaddingValues(horizontal = 10.dp, vertical = 4.dp)
