// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// android wave contract (docs/redesign/12 §3.9). Owner: worker b; called by worker c (Home). Name and signature FIXED.

/**
 * Hero multi-browse carousel (`HorizontalMultiBrowseCarousel` of androidx.compose.material3.carousel) with page dots and autoplay every
 * [autoplayMillis]; autoplay is off when [autoplay] is false OR `LocalReduceMotion.current`, and pauses while the user drags.
 * The caller draws each slide with [itemContent] (cover, title, actions). Body: worker b.
 */
@Composable
fun <T> HeroCarousel(
    items: List<T>,
    key: (T) -> Any,
    modifier: Modifier = Modifier,
    autoplay: Boolean = true,
    autoplayMillis: Long = 6_000,
    itemContent: @Composable (item: T, index: Int) -> Unit,
) {
    Text("HeroCarousel", modifier)
}
