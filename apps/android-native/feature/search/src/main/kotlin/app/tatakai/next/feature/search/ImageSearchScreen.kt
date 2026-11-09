// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.Media

/**
 * Parity wave A contract placeholder (docs/redesign/15-parity-wave-a-contract-android.md §3.1). Owner: worker o.
 *
 * Classic `/image-search` (trace.moe): pick an image (SAF `GetContent("image/*")`, or [imageUri] when the screen was opened by sharing a
 * picture to the app), send its bytes through `MetadataRepository.searchByImage`, show the matches best first (preview frame, title,
 * episode and timestamp, similarity) with a similarity threshold slider; tapping a match opens Details ([onMediaClick] with the
 * `Media` of `ImageMatch.ref`, resolved through `MetadataRepository.media`). Three states; errors inline.
 */
@Composable
fun ImageSearchScreen(
    onBack: () -> Unit,
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
    /** `content://` or `file://` of a shared image; null = let the user pick one. */
    imageUri: String? = null,
) {
    Text("ImageSearchScreen", modifier)
}
