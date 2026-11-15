package app.tatakai.next.feature.link

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.ProfileSupport

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

/** Public for the player's Link profile sheet (BUG-022). */
@Composable
fun profileLabel(profile: LinkProfile): String {
    val name = ProfileName.parse(profile.id) ?: return profile.label
    if (name.none) return stringResource(R.string.linked_profile_none)
    val parts = buildList {
        name.upscaleHeight?.let { add(stringResource(R.string.linked_profile_anime4k, it)) }
        name.multiplier?.let { add(stringResource(R.string.linked_profile_fg, it)) }
        name.autoFps?.let { add(stringResource(R.string.linked_profile_fg_auto, it)) }
    }
    return parts.joinToString(" + ")
}


/** BUG-132: why this phone cannot play a profile, for its disabled row ("Tu teléfono no reproduce 1440p"). */
@Composable
fun profileSupportText(support: ProfileSupport): String? = when (support) {
    ProfileSupport.Supported -> null
    is ProfileSupport.TooLarge -> stringResource(R.string.linked_profile_too_large, support.height)
    ProfileSupport.Rejected -> stringResource(R.string.linked_profile_rejected)
}

/**
 * Radio list of the PC's profiles: one row each with its full name, so a dozen options stay readable. BUG-132: the profiles in
 * [unavailable] (this phone's decoder cannot play them) are shown disabled, with the reason under the name. BUG-135: the ones in
 * [mayDropFrames] (frame rate above what the decoder declares) stay selectable, with a one-line note.
 */
@Composable
fun ProfileList(
    profiles: List<LinkProfile>,
    selected: String,
    unavailable: Map<String, ProfileSupport> = emptyMap(),
    mayDropFrames: Set<String> = emptySet(),
    onSelect: (String) -> Unit,
) {
    Column(Modifier.fillMaxWidth().selectableGroup()) {
        profiles.forEach { profile ->
            val on = profile.id == selected
            val reason = unavailable[profile.id]?.let { profileSupportText(it) }
            val enabled = reason == null
            val note = if (enabled && profile.id in mayDropFrames) stringResource(R.string.linked_profile_may_drop_frames) else null
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .selectable(selected = on, enabled = enabled, role = Role.RadioButton, onClick = { onSelect(profile.id) }),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = on, onClick = null, enabled = enabled)
                Column(Modifier.padding(start = 12.dp)) {
                    Text(
                        profileLabel(profile),
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalContentColor.current.let { if (enabled) it else it.copy(alpha = 0.38f) },
                    )
                    if (reason != null) {
                        Text(reason, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                    if (note != null) {
                        Text(
                            note,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
}
