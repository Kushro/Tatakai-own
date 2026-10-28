package app.tatakai.next.ui.state

import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.statusBars
import androidx.compose.runtime.Composable

/**
 * Insets that a top-level screen must apply itself: only the status bar. Bottom/side insets are consumed by the
 * navigation suite (bar / rail) that hosts the screen, so applying `safeDrawing` would double the padding.
 */
val screenInsets: WindowInsets
    @Composable get() = WindowInsets.statusBars
