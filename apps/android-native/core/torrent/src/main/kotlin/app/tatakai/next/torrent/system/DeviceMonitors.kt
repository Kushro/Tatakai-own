// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.system

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.os.BatteryManager
import android.os.PowerManager
import androidx.core.content.ContextCompat
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/** Default network facts (11 §7): `NET_CAPABILITY_NOT_METERED` decides `wifiOnly`; a VPN does not change the policy. */
data class NetworkState(val connected: Boolean, val metered: Boolean)

/** `ConnectivityManager.registerDefaultNetworkCallback` as a StateFlow. Registered once for the process (application context). */
class NetworkMonitor(context: Context) {
    private val cm = context.applicationContext.getSystemService(ConnectivityManager::class.java)
    private val _state = MutableStateFlow(initial())
    val state: StateFlow<NetworkState> = _state.asStateFlow()

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onCapabilitiesChanged(network: Network, caps: NetworkCapabilities) {
            _state.value = NetworkState(
                connected = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET),
                metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
            )
        }

        override fun onLost(network: Network) {
            _state.value = NetworkState(connected = false, metered = _state.value.metered)
        }
    }

    @Volatile private var registered = false

    fun register() {
        if (registered || cm == null) return
        try {
            cm.registerDefaultNetworkCallback(callback)
            registered = true
        } catch (_: Exception) {
            // too many callbacks / no permission: keep the initial snapshot
        }
    }

    private fun initial(): NetworkState {
        val manager = cm ?: return NetworkState(connected = true, metered = false)
        return try {
            val caps = manager.getNetworkCapabilities(manager.activeNetwork)
            if (caps == null) {
                NetworkState(connected = false, metered = manager.isActiveNetworkMetered)
            } else {
                NetworkState(
                    connected = caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET),
                    metered = !caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED),
                )
            }
        } catch (_: Exception) {
            NetworkState(connected = true, metered = false)
        }
    }
}

/**
 * Battery low for the torrent policy (11 §7): level < 20 % while not charging, or battery saver on. Sticky `ACTION_BATTERY_CHANGED`
 * + `ACTION_POWER_SAVE_MODE_CHANGED`.
 */
class BatteryMonitor(context: Context) {
    private val app = context.applicationContext
    private val power = app.getSystemService(PowerManager::class.java)
    private val _low = MutableStateFlow(false)
    val low: StateFlow<Boolean> = _low.asStateFlow()

    private var level = 100
    private var charging = true

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == Intent.ACTION_BATTERY_CHANGED) read(intent)
            recompute()
        }
    }

    @Volatile private var registered = false

    fun register() {
        if (registered) return
        try {
            val filter = IntentFilter().apply {
                addAction(Intent.ACTION_BATTERY_CHANGED)
                addAction(PowerManager.ACTION_POWER_SAVE_MODE_CHANGED)
            }
            // BATTERY_CHANGED is a protected system broadcast. NOT_EXPORTED makes ContextCompat add a
            // signature permission below API 33, so the system broadcast gets denied (seen on API 31).
            ContextCompat.registerReceiver(app, receiver, filter, ContextCompat.RECEIVER_EXPORTED)?.let(::read)
            registered = true
            recompute()
        } catch (_: Exception) {
            // keep "not low"
        }
    }

    private fun read(intent: Intent) {
        if (intent.action != Intent.ACTION_BATTERY_CHANGED) return
        val raw = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, 100).coerceAtLeast(1)
        if (raw >= 0) level = raw * 100 / scale
        val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, BatteryManager.BATTERY_STATUS_UNKNOWN)
        charging = status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
    }

    private fun recompute() {
        val saver = power?.isPowerSaveMode == true
        _low.value = saver || (!charging && level < LOW_LEVEL)
    }

    private companion object {
        const val LOW_LEVEL = 20
    }
}
