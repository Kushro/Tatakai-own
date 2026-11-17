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
 *
 * 0.4.0 (worker B): with phone frame generation the same SurfaceView is the FG output (the native presenter draws on it) while
 * mpv renders into the FG input; the callbacks go through the engine's [MpvFrameGenBridge] first, which says when the engine
 * must not resize / detach (mpv is not on this surface then). Without a bridge the behaviour is unchanged.
 */
@Composable
fun MpvPlayerSurface(engine: MpvEngine, modifier: Modifier = Modifier) {
    AndroidView(
        modifier = modifier,
        factory = { ctx ->
            SurfaceView(ctx).apply {
                val view = this
                holder.addCallback(object : SurfaceHolder.Callback {
                    override fun surfaceCreated(holder: SurfaceHolder) {
                        engine.attachSurface(holder.surface)
                        MpvFrameGenBridge.of(engine)?.onSurfaceCreated(view)
                    }

                    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
                        if (MpvFrameGenBridge.of(engine)?.onSurfaceChanged(width, height) != true) engine.resizeSurface(width, height)
                    }

                    override fun surfaceDestroyed(holder: SurfaceHolder) {
                        if (MpvFrameGenBridge.of(engine)?.onSurfaceDestroyed() != true) engine.detachSurface()
                    }
                })
            }
        },
    )
}
