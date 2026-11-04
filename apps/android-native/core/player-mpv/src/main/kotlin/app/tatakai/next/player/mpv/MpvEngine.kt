// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import kotlinx.coroutines.flow.StateFlow

// android wave contract (docs/redesign/12 §3.2). Owner: worker d. Only the names below are fixed for other workers (the app lists
// [mpvPlayerModule]; feature/player renders [MpvPlayerSurface]); everything else in this module is d's to shape.
// libmpv API: dev.jdtech.mpv.MPVLib instance API (MPVLib.create(context), init(), destroy(), attachSurface(Surface), detachSurface(),
// command(Array<String>), setOptionString, get/setProperty{Int,Double,Boolean,String}, observeProperty(name, MpvFormat), addObserver,
// addLogObserver) - see .deps-sources/libmpv-1.0.0/MPVLib.signatures.txt.

/** Minimal state the player UI renders for the mpv branch (d may extend it). */
data class MpvState(
    val playing: Boolean = false,
    val buffering: Boolean = true,
    val ended: Boolean = false,
    val positionSecs: Double = 0.0,
    val durationSecs: Double = 0.0,
    val error: String? = null,
)

/** One libmpv instance (create on the main thread, [release] exactly once). Bodies: worker d. */
class MpvEngine(context: Context) {
    val state: StateFlow<MpvState> get() = TODO("worker d")

    /** Load [url] with HTTP [headers] (`http-header-fields`), start at [startSecs]. */
    fun load(url: String, headers: Map<String, String>, startSecs: Double = 0.0): Unit = TODO("worker d")

    fun play(): Unit = TODO("worker d")

    fun pause(): Unit = TODO("worker d")

    fun seekTo(secs: Double): Unit = TODO("worker d")

    fun release(): Unit = TODO("worker d")
}
