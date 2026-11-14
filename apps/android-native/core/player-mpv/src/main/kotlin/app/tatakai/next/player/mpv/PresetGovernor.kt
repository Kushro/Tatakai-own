// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.player.ShaderFallback

// BUG-043 / BUG-006: keeps the shader preset within what the phone can do. Pure (no Android), unit tested; MpvPlayerSession feeds it.
//
// - Battery: "Anime4K en el teléfono consume batería; se desactiva por debajo del 20 %" (Settings › Reproductor). Below 20 % and not
//   charging an Anime4K chain is not applied (or is switched off while playing).
// - Real time: mpv's dropped-frame counters are sampled while playing; when more than [MAX_DROP_RATIO] of the frames are dropped over
//   [WINDOWS_TO_DEGRADE] consecutive windows, the chain steps down (full -> lite -> off), never back up during the session.

data class BatteryInfo(val percent: Int, val charging: Boolean) {
    /** The rule of the setting: an Anime4K chain is not used below 20 % unless the phone charges. */
    val low: Boolean get() = !charging && percent in 0 until LOW_BATTERY_PERCENT

    companion object {
        const val LOW_BATTERY_PERCENT = 20
    }
}

class PresetGovernor(requested: String) {
    /** The preset applied now. */
    var preset: String = requested
        private set
    private var lastDrops: Long? = null
    private var lastTimeMs: Long = 0
    private var slowWindows = 0

    /** The preset to apply at start for [battery] (null = unknown battery: as requested). */
    fun initial(battery: BatteryInfo?): Pair<String, ShaderFallback?> {
        if (battery?.low == true && ShaderPresets.isAnime4K(preset)) {
            preset = "off"
            return preset to ShaderFallback.OFF_BATTERY
        }
        return preset to null
    }

    /** Periodic battery reading while playing: switches an Anime4K chain off when the battery goes low. */
    fun onBattery(battery: BatteryInfo?): ShaderFallback? {
        if (battery?.low != true || !ShaderPresets.isAnime4K(preset)) return null
        preset = "off"
        return ShaderFallback.OFF_BATTERY
    }

    /**
     * One sample while playing: [droppedFrames] = mpv `frame-drop-count` + `decoder-frame-drop-count` (cumulative), [fps] = the video's
     * frame rate, [nowMs] a monotonic clock. Returns the change when the chain steps down (read [preset] for the new one).
     */
    fun onSample(droppedFrames: Long, fps: Double, nowMs: Long): ShaderFallback? {
        if (ShaderPresets.chain(preset).isEmpty()) return null
        val prev = lastDrops
        val prevTime = lastTimeMs
        lastDrops = droppedFrames
        lastTimeMs = nowMs
        if (prev == null || fps <= 0.0) return null
        val elapsedMs = nowMs - prevTime
        if (elapsedMs < MIN_WINDOW_MS) return null
        val expected = fps * elapsedMs / 1000.0
        val ratio = (droppedFrames - prev).coerceAtLeast(0) / expected
        slowWindows = if (ratio > MAX_DROP_RATIO) slowWindows + 1 else 0
        if (slowWindows < WINDOWS_TO_DEGRADE) return null
        slowWindows = 0
        lastDrops = null // the new chain starts a fresh measurement
        val next = ShaderPresets.lighter(preset) ?: return null
        preset = next
        return if (next == "off") ShaderFallback.OFF_SLOW else ShaderFallback.LIGHTER
    }

    /** A seek / pause / new file: drops counted across it are not the chain's fault. */
    fun reset() {
        lastDrops = null
        slowWindows = 0
    }

    companion object {
        const val MAX_DROP_RATIO = 0.25
        const val WINDOWS_TO_DEGRADE = 2
        const val MIN_WINDOW_MS = 1_500L
        const val SAMPLE_MS = 2_000L
    }
}
