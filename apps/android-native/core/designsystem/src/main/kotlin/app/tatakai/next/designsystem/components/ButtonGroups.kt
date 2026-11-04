@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.designsystem.components

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp

/** Single-choice selector built on the M3E [ButtonGroup] + [ToggleButton] (contrast level, ...). */
@Composable
fun <T> SingleChoiceButtonGroup(
    options: List<T>,
    selected: T,
    label: @Composable (T) -> String,
    onSelected: (T) -> Unit,
    modifier: Modifier = Modifier,
) {
    ButtonGroup(modifier = modifier) {
        options.forEach { option ->
            val interaction = remember { MutableInteractionSource() }
            ToggleButton(
                checked = option == selected,
                onCheckedChange = { checked -> if (checked) onSelected(option) },
                modifier = Modifier.animateWidth(interaction),
                interactionSource = interaction,
            ) {
                Text(label(option))
            }
        }
    }
}

/**
 * Floating action bar for Details / Reader actions (01 §6). Drawn with a Surface + Row in the look of the M3E floating toolbar instead of
 * `HorizontalFloatingToolbar`: material3 1.5.0-alpha15's MinimumInteractiveBalancedPaddingNode throws a NullPointerException in the
 * lookahead pass of the NavDisplay transition when Details comes back from the player (seen on a moto g51, API 31). [expanded] = false
 * hides the bar.
 */
@Composable
fun TatakaiFloatingToolbar(
    modifier: Modifier = Modifier,
    expanded: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    if (!expanded) return
    Surface(
        modifier = modifier,
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainer,
        contentColor = MaterialTheme.colorScheme.onSurface,
        tonalElevation = 3.dp,
        shadowElevation = 6.dp,
    ) {
        Row(
            modifier = Modifier.padding(horizontal = 8.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
            content = content,
        )
    }
}
