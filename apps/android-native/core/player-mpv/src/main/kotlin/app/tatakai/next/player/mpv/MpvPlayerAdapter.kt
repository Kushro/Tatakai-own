// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.os.Looper
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.2, 2026-11-09 amendment). Owner: worker p.
 *
 * libmpv seen as a Media3 `Player`, so `PlaybackService` (:core:player) publishes one `MediaSession` for both engines. Worker p fills
 * [getState] from [MpvEngine.state] (playWhenReady = !paused, playbackState from buffering / ended / error, position and duration
 * from `time-pos` / `duration`, one `MediaItemData` with `PlaybackMetadata`) and overrides `handleSetPlayWhenReady`, `handleSeek`,
 * `handleStop`, `handleRelease` to drive the engine, calling `invalidateState()` on every engine update. Reference for the
 * observe-the-core pattern, the per-core re-registration and the "no update after shutdown" gate: Enigmata
 * `app/src/main/java/app/enigmata/ui/player/MediaPlaybackService.kt` (same author, AGPL-3.0-or-later; copying allowed with the header
 * and a THIRD_PARTY_NOTICES.md row). `MpvPlayerSession` publishes this adapter through `ActivePlayback.publish` on load and null on release.
 * Until then the state is empty (no commands available), which Media3 accepts.
 */
@UnstableApi
class MpvPlayerAdapter(private val engine: MpvEngine, looper: Looper) : SimpleBasePlayer(looper) {
    override fun getState(): State = State.Builder().build()
}
