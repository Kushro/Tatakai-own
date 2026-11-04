// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.net.wifi.WifiManager
import android.os.Build
import android.os.PowerManager
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import app.tatakai.next.torrent.system.ServiceSnapshot
import app.tatakai.next.torrent.system.TorrentNotifications
import java.util.Locale
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import org.koin.core.context.GlobalContext

/**
 * Sole owner of the torrent session while anything streams or downloads (11 §5.2). Foreground type `mediaPlayback` while a loopback
 * stream is open, `dataSync` for background downloads; Android 15 `onTimeout` pauses everything and posts "Descargas pausadas por el
 * sistema". Notification channel id [CHANNEL_ID]. Declared in this module's AndroidManifest.xml.
 *
 * The engine lives in [TorrentRepositoryImpl] (Koin singleton); this service keeps the process in the foreground, shows the
 * aggregated notification (Pausar todo / Abrir), holds the Wi-Fi / wake locks only while data actually moves and stops itself 30 s
 * after nothing needs it.
 */
class TorrentService : LifecycleService() {
    companion object {
        const val CHANNEL_ID: String = "torrents"

        internal const val ACTION_PAUSE_ALL: String = "app.tatakai.next.torrent.action.PAUSE_ALL"
        internal const val ACTION_RESUME_ALL: String = "app.tatakai.next.torrent.action.RESUME_ALL"
        private const val IDLE_STOP_MS = 30_000L
        private const val LOCK_TAG = "tatakai:torrents"

        /** Starts (or pokes) the service; silently ignored when Android forbids a foreground start from the background. */
        internal fun start(context: Context) {
            try {
                ContextCompat.startForegroundService(context, Intent(context, TorrentService::class.java))
            } catch (_: Exception) {
                // ForegroundServiceStartNotAllowedException (Android 12+): the engine keeps working while the app is visible and the
                // next foreground action starts the service again.
            }
        }

        /** "4.2 MB/s" (decimal units, same output as :core:ui formatRate; duplicated to keep this module free of Compose). */
        internal fun rate(bytesPerSec: Long): String {
            if (bytesPerSec < 1000) return "${bytesPerSec.coerceAtLeast(0)} B/s"
            val units = arrayOf("B", "kB", "MB", "GB", "TB")
            var v = bytesPerSec.toDouble()
            var u = 0
            while (v >= 1000 && u < units.lastIndex) {
                v /= 1000
                u++
            }
            return String.format(Locale.ROOT, if (v >= 100) "%.0f %s/s" else "%.1f %s/s", v, units[u])
        }
    }

    private val repository: TorrentRepositoryImpl? by lazy {
        runCatching { GlobalContext.get().get<TorrentRepositoryImpl>() }.getOrNull()
    }

    private var foregroundType = -1
    private var idleJob: Job? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private var wifiLock: WifiManager.WifiLock? = null
    private var stopping = false

    override fun onCreate() {
        super.onCreate()
        TorrentNotifications.ensureChannel(this)
        val repo = repository ?: return
        repo.serviceRunning = true
        goForeground(repo.serviceSnapshot.value)
        lifecycleScope.launch { repo.serviceSnapshot.collect { onSnapshot(it) } }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        val repo = repository
        if (repo == null) {
            // still satisfy the startForeground contract before leaving
            goForeground(ServiceSnapshot())
            stopNow()
            return START_NOT_STICKY
        }
        // startForegroundService requires startForeground within a few seconds of every start
        goForeground(repo.serviceSnapshot.value)
        when (intent?.action) {
            ACTION_PAUSE_ALL -> repo.pauseAll()
            ACTION_RESUME_ALL -> repo.resumeAfterSystemPause()
        }
        return START_NOT_STICKY
    }

    /** Android 14: time limit of the foreground service type reached. */
    override fun onTimeout(startId: Int) {
        handleTimeout()
    }

    /** Android 15+: time limit of the foreground service type reached (dataSync: about 6 h per 24 h). */
    override fun onTimeout(startId: Int, fgsType: Int) {
        handleTimeout()
    }

    override fun onDestroy() {
        releaseLocks()
        repository?.serviceRunning = false
        super.onDestroy()
    }

    private fun handleTimeout() {
        repository?.pauseBySystem()
        TorrentNotifications.notifyPausedBySystem(this)
        stopNow()
    }

    private fun onSnapshot(snapshot: ServiceSnapshot) {
        if (stopping) return
        goForeground(snapshot)
        if (snapshot.streaming || snapshot.downBps > 0 || snapshot.upBps > 0) acquireLocks(snapshot.streaming) else releaseLocks()
        if (snapshot.demand) {
            idleJob?.cancel()
            idleJob = null
        } else if (idleJob == null) {
            idleJob = lifecycleScope.launch {
                delay(IDLE_STOP_MS)
                stopNow()
            }
        }
    }

    @SuppressLint("MissingPermission")
    private fun goForeground(snapshot: ServiceSnapshot) {
        if (stopping) return
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            if (snapshot.streaming) ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PLAYBACK else ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
        } else {
            0
        }
        val notification = TorrentNotifications.foreground(this, snapshot) { rate(it) }
        try {
            if (type != foregroundType) {
                ServiceCompat.startForeground(this, TorrentNotifications.FOREGROUND_ID, notification, type)
                foregroundType = type
            } else {
                val manager = NotificationManagerCompat.from(this)
                if (manager.areNotificationsEnabled()) manager.notify(TorrentNotifications.FOREGROUND_ID, notification)
            }
        } catch (_: SecurityException) {
            // notification permission revoked: the foreground state stays
        } catch (_: Exception) {
            // e.g. dataSync not allowed any more after onTimeout, or a background start restriction
            stopNow()
        }
    }

    private fun stopNow() {
        if (stopping) return
        stopping = true
        idleJob?.cancel()
        releaseLocks()
        runCatching { ServiceCompat.stopForeground(this, ServiceCompat.STOP_FOREGROUND_REMOVE) }
        stopSelf()
    }

    @SuppressLint("WakelockTimeout")
    private fun acquireLocks(streaming: Boolean) {
        if (wakeLock == null) {
            wakeLock = runCatching {
                getSystemService(PowerManager::class.java)?.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, LOCK_TAG)?.apply {
                    setReferenceCounted(false)
                    acquire()
                }
            }.getOrNull()
        }
        if (wifiLock == null) {
            val mode = if (streaming && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                WifiManager.WIFI_MODE_FULL_LOW_LATENCY
            } else {
                @Suppress("DEPRECATION")
                WifiManager.WIFI_MODE_FULL_HIGH_PERF
            }
            wifiLock = runCatching {
                applicationContext.getSystemService(WifiManager::class.java)?.createWifiLock(mode, LOCK_TAG)?.apply {
                    setReferenceCounted(false)
                    acquire()
                }
            }.getOrNull()
        }
    }

    private fun releaseLocks() {
        runCatching { wakeLock?.takeIf { it.isHeld }?.release() }
        runCatching { wifiLock?.takeIf { it.isHeld }?.release() }
        wakeLock = null
        wifiLock = null
    }
}
