// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.Media

/**
 * Parity wave A contract placeholder (docs/redesign/15-parity-wave-a-contract-android.md §3.1). Owner: worker o.
 *
 * Classic `/character/:name` page: image, name + native name, facts (age, birthday, blood type, gender, favourites), description with
 * More / Less, voice actors, appearances grid ([onMediaClick] -> Details). Data: `MetadataRepository.character(id)`. Rendered in the
 * detail pane on expanded windows (same `ListDetailSceneStrategy.detailPane()` as Details). Three states.
 */
@Composable
fun CharacterScreen(
    characterId: Int,
    onBack: () -> Unit,
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("CharacterScreen", modifier)
}
