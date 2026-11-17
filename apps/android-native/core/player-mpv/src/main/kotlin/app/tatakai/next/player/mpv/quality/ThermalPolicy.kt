// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import android.content.Context
import android.os.Build
import android.os.PowerManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Adapted from Enigmata `app/src/main/java/app/enigmata/domain/quality/ThermalPolicy.kt` (AGPL-3.0-or-later, same author): package
// renamed, the watcher's callback feeds `MpvQualityController.onThermal`. Logic unchanged.

/**
 * Pure thermal-degrade decision with hysteresis. `status`: `PowerManager.getCurrentThermalStatus()` (0 NONE .. 6 SHUTDOWN), null = no
 * data. `headroom`: `PowerManager.getThermalHeadroom()` (1.0 = the SEVERE threshold, higher = hotter; NaN / null = no data).
 *
 * Enter when status >= SEVERE or headroom >= [ENTER_HEADROOM]; leave only when status < MODERATE (or unknown) and headroom is unknown
 * or < [EXIT_HEADROOM]. In between the previous verdict stays, so the tier does not flap around one threshold.
 */
object ThermalPolicy {
    const val STATUS_MODERATE = 2
    const val STATUS_SEVERE = 3
    const val ENTER_HEADROOM = 0.95f
    const val EXIT_HEADROOM = 0.80f

    fun isDegraded(status: Int?, headroom: Float?, wasDegraded: Boolean): Boolean {
        val h = headroom?.takeUnless { it.isNaN() }
        val enter = (status != null && status >= STATUS_SEVERE) || (h != null && h >= ENTER_HEADROOM)
        if (enter) return true
        val statusCool = status == null || status < STATUS_MODERATE
        val headroomCool = h == null || h < EXIT_HEADROOM
        if (statusCool && headroomCool) return false
        return wasDegraded
    }
}

/**
 * Platform side of [ThermalPolicy]: a thermal-status listener (API 29+) plus a headroom sample every [SAMPLE_INTERVAL_MS] (API 30+,
 * well above the one-call-per-second limit that would yield NaN). [onChange] fires only when the verdict flips, on the main executor /
 * [scope]'s dispatcher. No-op below API 29 (the moto g51 runs Android 12, so it is covered).
 */
class ThermalWatcher(context: Context, private val scope: CoroutineScope, private val onChange: (Boolean) -> Unit) {
    private val appContext = context.applicationContext
    private val powerManager: PowerManager? = appContext.getSystemService(PowerManager::class.java)
    private var lastStatus: Int? = null
    private var lastHeadroom: Float? = null
    private var degraded = false
    private var started = false
    private var sampleJob: Job? = null

    private val statusListener: Any? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) PowerManager.OnThermalStatusChangedListener { onStatus(it) } else null

    fun start() {
        if (started || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        val pm = powerManager ?: return
        val listener = statusListener as? PowerManager.OnThermalStatusChangedListener ?: return
        runCatching { pm.addThermalStatusListener(appContext.mainExecutor, listener) }.onFailure { return }
        started = true
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            sampleJob = scope.launch {
                while (isActive) {
                    val headroom = withContext(Dispatchers.Default) {
                        runCatching { pm.getThermalHeadroom(HEADROOM_FORECAST_SECONDS) }.getOrNull()
                    }
                    onHeadroom(headroom)
                    delay(SAMPLE_INTERVAL_MS)
                }
            }
        }
    }

    fun stop() {
        if (!started || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return
        sampleJob?.cancel()
        sampleJob = null
        (statusListener as? PowerManager.OnThermalStatusChangedListener)?.let { l ->
            runCatching { powerManager?.removeThermalStatusListener(l) }
        }
        started = false
    }

    private fun onStatus(status: Int) {
        lastStatus = status
        evaluate()
    }

    private fun onHeadroom(headroom: Float?) {
        lastHeadroom = headroom
        evaluate()
    }

    private fun evaluate() {
        val next = ThermalPolicy.isDegraded(lastStatus, lastHeadroom, degraded)
        if (next == degraded) return
        degraded = next
        onChange(next)
    }

    private companion object {
        const val SAMPLE_INTERVAL_MS = 10_000L
        const val HEADROOM_FORECAST_SECONDS = 10
    }
}
