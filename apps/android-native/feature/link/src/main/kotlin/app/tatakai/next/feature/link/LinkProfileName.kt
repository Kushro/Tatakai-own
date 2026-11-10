package app.tatakai.next.feature.link

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.LinkProfile

/**
 * What a Link profile does, read from its stable id (`contracts/link`: `p-none`, `p-anime4k-<h>`, `p-anime4k-1080-fg2x`,
 * `p-lsfg-1080-x<n>`, `p-lsfg-1080-auto<fps>`, `p-anime4k-lsfg-1080-…`). The server's `label` is English-only, so the
 * phone names the profile itself and only falls back to `label` for ids it does not know.
 */
internal data class ProfileName(
    val upscaleHeight: Int? = null,
    /** Fixed frame-generation multiplier (LSFG ×n or the AMD ×2). */
    val multiplier: Int? = null,
    /** LSFG automatic multiplier aiming at this frame rate. */
    val autoFps: Int? = null,
) {
    val none: Boolean get() = upscaleHeight == null && multiplier == null && autoFps == null

    companion object {
        private val PATTERN = Regex("""^p-(anime4k-)?(lsfg-)?(\d{3,4})(?:-x(\d)|-auto(\d{2,3})|-fg(\d)x)?$""")

        fun parse(id: String): ProfileName? {
            if (id == "p-none") return ProfileName()
            val m = PATTERN.matchEntire(id) ?: return null
            val (anime4k, lsfg, height, x, auto, fg) = m.destructured
            if (anime4k.isEmpty() && lsfg.isEmpty()) return null
            if (lsfg.isNotEmpty() && x.isEmpty() && auto.isEmpty()) return null
            return ProfileName(
                upscaleHeight = height.toInt().takeIf { anime4k.isNotEmpty() },
                multiplier = x.toIntOrNull() ?: fg.toIntOrNull(),
                autoFps = auto.toIntOrNull(),
            )
        }
    }
}

@Composable
internal fun profileLabel(profile: LinkProfile): String {
    val name = ProfileName.parse(profile.id) ?: return profile.label
    if (name.none) return stringResource(R.string.linked_profile_none)
    val parts = buildList {
        name.upscaleHeight?.let { add(stringResource(R.string.linked_profile_anime4k, it)) }
        name.multiplier?.let { add(stringResource(R.string.linked_profile_fg, it)) }
        name.autoFps?.let { add(stringResource(R.string.linked_profile_fg_auto, it)) }
    }
    return parts.joinToString(" + ")
}


/** Radio list of the PC's profiles: one row each with its full name, so a dozen options stay readable. */
@Composable
internal fun ProfileList(profiles: List<LinkProfile>, selected: String, onSelect: (String) -> Unit) {
    Column(Modifier.fillMaxWidth().selectableGroup()) {
        profiles.forEach { profile ->
            val on = profile.id == selected
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .selectable(selected = on, role = Role.RadioButton, onClick = { onSelect(profile.id) }),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = on, onClick = null)
                Text(
                    profileLabel(profile),
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(start = 12.dp),
                )
            }
        }
    }
}
