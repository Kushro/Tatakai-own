// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.designsystem.components

import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.carousel.HorizontalMultiBrowseCarousel
import androidx.compose.material3.carousel.rememberCarouselState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.graphics.Color
import app.tatakai.next.designsystem.R
import app.tatakai.next.designsystem.theme.LocalReduceMotion
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// android wave contract (docs/redesign/12 §3.9). Owner: worker b; called by worker c (Home). Name and signature FIXED.

/**
 * Hero multi-browse carousel (`HorizontalMultiBrowseCarousel` of androidx.compose.material3.carousel) with page dots and autoplay every
 * [autoplayMillis]; autoplay is off when [autoplay] is false OR `LocalReduceMotion.current`, and pauses while the user drags.
 * The caller draws each slide with [itemContent] (cover, title, actions).
 *
 * Slide size: the item is clipped with `maskClip(extraLarge)`; its height is 16:10 of the preferred width (max 420 dp), the caller fills it.
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
    if (items.isEmpty()) return
    val currentItems by rememberUpdatedState(items)
    val state = rememberCarouselState(itemCount = { currentItems.size })
    val reduceMotion = LocalReduceMotion.current
    val scope = rememberCoroutineScope()
    val dragging = state.isScrollInProgress
    val current = state.currentItem

    // Restarts on every page change and on drag start/end, so a drag pauses the timer and the next slide waits a full period.
    LaunchedEffect(autoplay, reduceMotion, autoplayMillis, items.size, current, dragging) {
        if (!autoplay || reduceMotion || dragging || items.size < 2) return@LaunchedEffect
        delay(autoplayMillis.coerceAtLeast(1_000))
        val next = (state.currentItem + 1) % items.size
        state.animateScrollToItem(next)
    }

    Column(modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        BoxWithConstraints(Modifier.fillMaxWidth()) {
            val preferred = (maxWidth - 48.dp).coerceIn(160.dp, 720.dp)
            val height = (preferred * 0.625f).coerceAtMost(420.dp)
            HorizontalMultiBrowseCarousel(
                state = state,
                preferredItemWidth = preferred,
                modifier = Modifier.fillMaxWidth().height(height),
                itemSpacing = 8.dp,
                contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 16.dp),
            ) { index ->
                val item = items.getOrNull(index) ?: return@HorizontalMultiBrowseCarousel
                Box(Modifier.fillMaxWidth().height(height).maskClip(MaterialTheme.shapes.extraLarge)) {
                    androidx.compose.runtime.key(key(item)) { itemContent(item, index) }
                }
            }
        }
        if (items.size > 1) {
            Row(
                Modifier.padding(top = 4.dp),
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                val pageLabel = stringResource(R.string.ds_carousel_page, current + 1, items.size)
                items.indices.forEach { i ->
                    val selected = i == current
                    val goTo = stringResource(R.string.ds_carousel_go_to, i + 1)
                    val width by animateDpAsState(
                        targetValue = if (selected) 20.dp else 8.dp,
                        animationSpec = if (reduceMotion) snap<Dp>() else MaterialTheme.motionScheme.fastSpatialSpec<Dp>(),
                        label = "dotWidth",
                    )
                    val color by animateColorAsState(
                        targetValue = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outlineVariant,
                        animationSpec = if (reduceMotion) snap<Color>() else MaterialTheme.motionScheme.fastEffectsSpec<Color>(),
                        label = "dotColor",
                    )
                    Box(
                        Modifier
                            .size(width = 24.dp, height = 24.dp)
                            .clip(CircleShape)
                            .clickable { scope.launch { if (reduceMotion) state.scrollToItem(i) else state.animateScrollToItem(i) } }
                            .semantics { contentDescription = if (selected) pageLabel else goTo },
                        contentAlignment = Alignment.Center,
                    ) {
                        Box(Modifier.width(width).height(8.dp).clip(CircleShape).background(color))
                    }
                }
            }
        }
    }
}
