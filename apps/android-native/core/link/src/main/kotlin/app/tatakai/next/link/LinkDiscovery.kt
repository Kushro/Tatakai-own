// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import android.app.Activity
import android.app.ActivityManager
import android.app.Application
import android.content.Context
import android.net.nsd.NsdManager
import android.net.nsd.NsdServiceInfo
import android.os.Bundle
import app.tatakai.next.model.DiscoveredPc
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.callbackFlow
import java.net.Inet4Address
import java.net.URLEncoder
import java.util.Locale

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.11 / §7.4, worker r; checklist link.discovery): the PC announces
// `_tatakai-link._tcp` over mDNS (desktop tk-link/src/discovery.rs, CANONICAL: TXT `v=1`, `name`, `id` = instance_id, `fp` = sha256 hex
// of the pinned certificate). The phone lists what it finds; "Emparejar" asks for the one-time code the PC shows, shows the
// fingerprint first (TOFU) and runs the normal pairing with a `tatakai-link:v1` payload built from the announcement.

/** One PC found on the network: the public [pc] plus the TXT fingerprint used as the pin. */
data class FoundPc(val pc: DiscoveredPc, val fingerprint: String)

/** Source of announced PCs (NsdManager on device, fakes in tests). */
fun interface PcDiscovery {
    fun pcs(): Flow<List<FoundPc>>
}

/** Pure parsing / payload rules of the discovery (JVM-tested). */
object DiscoveryWire {
    const val SERVICE_TYPE = "_tatakai-link._tcp"
    const val TXT_VERSION = "v"
    const val TXT_NAME = "name"
    const val TXT_INSTANCE_ID = "id"
    const val TXT_FINGERPRINT = "fp"
    private val HEX64 = Regex("[0-9a-fA-F]{64}")
    private val IPV4 = Regex("""\d{1,3}(\.\d{1,3}){3}""")

    /** A resolved announcement, or null when it is not a v1 Tatakai Link with an IPv4 host and a valid fingerprint. */
    fun parse(serviceName: String, host: String?, port: Int, txt: Map<String, String?>): FoundPc? {
        if (txt[TXT_VERSION] != "1") return null
        val fp = txt[TXT_FINGERPRINT]?.takeIf { HEX64.matches(it) }?.lowercase(Locale.ROOT) ?: return null
        val h = host?.takeIf { IPV4.matches(it) } ?: return null
        if (port !in 1..65535) return null
        val name = txt[TXT_NAME]?.takeIf { it.isNotBlank() } ?: serviceName
        return FoundPc(DiscoveredPc(name = name.take(64), host = h, port = port, instanceId = txt[TXT_INSTANCE_ID]?.takeIf { it.isNotBlank() }), fp)
    }

    /** The pairing payload of a discovered PC + the code the user typed (normalised); null when the code is not 10 Crockford symbols. */
    fun payload(found: FoundPc, code: String, nowEpochSeconds: Long): String? {
        val normalized = PairingPayload.normalizeCode(code) ?: return null
        val n = URLEncoder.encode(found.pc.name, "UTF-8").replace("+", "%20")
        return "${PairingPayload.PREFIX}h=${found.pc.host}&p=${found.pc.port}&ca=${found.fingerprint}&c=$normalized&x=${nowEpochSeconds + PAYLOAD_TTL_SECS}&n=$n"
    }

    /** Display form of the fingerprint (`7A3F 91C2 …`). */
    fun displayFingerprint(fp: String): String = fp.uppercase(Locale.ROOT).chunked(4).joinToString(" ")

    private const val PAYLOAD_TTL_SECS = 300L
}

/** [PcDiscovery] over Android's `NsdManager` (multicast DNS-SD, no extra permission). Resolves one service at a time. */
class NsdPcDiscovery(private val context: Context) : PcDiscovery {
    override fun pcs(): Flow<List<FoundPc>> = callbackFlow {
        val nsd = context.getSystemService(Context.NSD_SERVICE) as? NsdManager
        if (nsd == null) {
            trySend(emptyList())
            awaitClose { }
            return@callbackFlow
        }
        val found = LinkedHashMap<String, FoundPc>()
        val pending = ArrayDeque<NsdServiceInfo>()
        var resolving = false
        val lock = Any()

        fun publish() = trySend(synchronized(lock) { found.values.toList() })

        fun next() {
            val info = synchronized(lock) {
                if (resolving) return
                pending.removeFirstOrNull()?.also { resolving = true }
            } ?: return
            @Suppress("DEPRECATION")
            nsd.resolveService(
                info,
                object : NsdManager.ResolveListener {
                    override fun onResolveFailed(serviceInfo: NsdServiceInfo, errorCode: Int) {
                        synchronized(lock) { resolving = false }
                        next()
                    }

                    override fun onServiceResolved(serviceInfo: NsdServiceInfo) {
                        @Suppress("DEPRECATION")
                        val host = (serviceInfo.host as? Inet4Address)?.hostAddress
                        val txt = serviceInfo.attributes.mapValues { (_, v) -> v?.let { String(it, Charsets.UTF_8) } }
                        val pc = DiscoveryWire.parse(serviceInfo.serviceName, host, serviceInfo.port, txt)
                        synchronized(lock) {
                            if (pc != null) found[serviceInfo.serviceName] = pc
                            resolving = false
                        }
                        publish()
                        next()
                    }
                },
            )
        }

        val listener = object : NsdManager.DiscoveryListener {
            override fun onStartDiscoveryFailed(serviceType: String, errorCode: Int) {
                publish()
            }

            override fun onStopDiscoveryFailed(serviceType: String, errorCode: Int) = Unit
            override fun onDiscoveryStarted(serviceType: String) {
                publish()
            }

            override fun onDiscoveryStopped(serviceType: String) = Unit

            override fun onServiceFound(serviceInfo: NsdServiceInfo) {
                synchronized(lock) { pending.addLast(serviceInfo) }
                next()
            }

            override fun onServiceLost(serviceInfo: NsdServiceInfo) {
                synchronized(lock) { found.remove(serviceInfo.serviceName) }
                publish()
            }
        }
        runCatching { nsd.discoverServices(DiscoveryWire.SERVICE_TYPE, NsdManager.PROTOCOL_DNS_SD, listener) }
            .onFailure { trySend(emptyList()) }
        awaitClose { runCatching { nsd.stopServiceDiscovery(listener) } }
    }
}

/** True while an activity of the app is started (the Link events long-poll only runs then: no background service). */
class AppForeground(app: Application) {
    private val _visible = MutableStateFlow(isForegroundNow())
    val visible: StateFlow<Boolean> = _visible.asStateFlow()
    private var started = if (_visible.value) 1 else 0

    init {
        app.registerActivityLifecycleCallbacks(
            object : Application.ActivityLifecycleCallbacks {
                override fun onActivityStarted(activity: Activity) {
                    started++
                    _visible.value = true
                }

                override fun onActivityStopped(activity: Activity) {
                    started = (started - 1).coerceAtLeast(0)
                    if (started == 0) _visible.value = false
                }

                override fun onActivityCreated(activity: Activity, savedInstanceState: Bundle?) = Unit
                override fun onActivityResumed(activity: Activity) = Unit
                override fun onActivityPaused(activity: Activity) = Unit
                override fun onActivitySaveInstanceState(activity: Activity, outState: Bundle) = Unit
                override fun onActivityDestroyed(activity: Activity) = Unit
            },
        )
    }

    private fun isForegroundNow(): Boolean = runCatching {
        val info = ActivityManager.RunningAppProcessInfo()
        ActivityManager.getMyMemoryState(info)
        info.importance <= ActivityManager.RunningAppProcessInfo.IMPORTANCE_VISIBLE
    }.getOrDefault(false)
}
