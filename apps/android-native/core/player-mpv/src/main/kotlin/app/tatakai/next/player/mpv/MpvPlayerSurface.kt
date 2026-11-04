// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.view.SurfaceHolder
import android.view.SurfaceView
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

/**
 * The video surface of the mpv branch (SurfaceView -> MPVLib.attachSurface). OUR controls are drawn on top by feature/player. The surface
 * is attached while it exists (mpv switches to `vo=null` when it is destroyed, e.g. in the background) and resized with
 * `android-surface-size`.
 */
@Composable
fun MpvPlayerSurface(engine: MpvEngine, modifier: Modifier = Modifier) {
    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            SurfaceView(ctx).apply {
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(holder: SurfaceHolder) = engine.attachSurface(holder.surface)

                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) =
                        engine.resizeSurface(width, height)

                    override fun surfaceDestroyed(holder: SurfaceHolder) = engine.detachSurface()
                })
            }
        },
    )
}
