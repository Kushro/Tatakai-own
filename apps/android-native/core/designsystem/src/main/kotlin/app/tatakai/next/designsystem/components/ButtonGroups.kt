@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.designsystem.components

import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.RowScope
import androidx.compose.material3.ButtonGroup
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.HorizontalFloatingToolbar
import androidx.compose.material3.Text
import androidx.compose.material3.ToggleButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier

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

/** Floating action bar for Details / Reader actions (01 §6). */
@Composable
fun TatakaiFloatingToolbar(
    modifier: Modifier = Modifier,
    expanded: Boolean = true,
    content: @Composable RowScope.() -> Unit,
) {
    HorizontalFloatingToolbar(expanded = expanded, modifier = modifier, content = content)
}
