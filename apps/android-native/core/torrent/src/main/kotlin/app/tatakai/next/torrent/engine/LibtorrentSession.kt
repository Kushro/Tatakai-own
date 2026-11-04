// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.engine

import app.tatakai.next.model.torrent.TorrentSettings
import java.io.File
import java.util.concurrent.atomic.AtomicInteger
import org.libtorrent4j.AddTorrentParams
import org.libtorrent4j.AlertListener
import org.libtorrent4j.ErrorCode
import org.libtorrent4j.SessionHandle
import org.libtorrent4j.SessionManager
import org.libtorrent4j.SessionParams
import org.libtorrent4j.SettingsPack
import org.libtorrent4j.Sha1Hash
import org.libtorrent4j.TorrentFlags
import org.libtorrent4j.TorrentHandle
import org.libtorrent4j.Vectors
import org.libtorrent4j.alerts.Alert
import org.libtorrent4j.alerts.AlertType
import org.libtorrent4j.alerts.SaveResumeDataAlert
import org.libtorrent4j.swig.error_code
import org.libtorrent4j.swig.libtorrent
import org.libtorrent4j.swig.remove_flags_t
import org.libtorrent4j.swig.settings_pack
import org.libtorrent4j.swig.torrent_flags_t

/**
 * Thin wrapper over libtorrent4j 2.1.0-39 `SessionManager` (docs/redesign/12 §4 f.1). Every libtorrent call of :core:torrent goes
 * through this class or [TorrentHandle] (APIs checked in .deps-sources/libtorrent4j-2.1.0-39). Blocking: call from Dispatchers.IO.
 *
 * Persistence (filesDir/torrents): `session.dat` (session state: DHT nodes), `<infoHash>.resume` (fastresume incl. the info dict).
 */
internal class LibtorrentSession(
    private val stateDir: File,
    private val onAlert: (Alert<*>) -> Unit,
) {
    private val manager = SessionManager(false)
    private val pendingResume = AtomicInteger()
    @Volatile private var lastListenPort: Int? = null

    private val listener = object : AlertListener {
        override fun types(): IntArray = LISTENED.map { it.swig() }.toIntArray()

        override fun alert(alert: Alert<*>) {
            if (alert.type() == AlertType.SAVE_RESUME_DATA) {
                writeResume(alert as SaveResumeDataAlert)
                pendingResume.decrementAndGet()
            } else if (alert.type() == AlertType.SAVE_RESUME_DATA_FAILED) {
                pendingResume.decrementAndGet()
            }
            try {
                onAlert(alert)
            } catch (_: Exception) {
                // never let a listener exception kill the alert loop
            }
        }
    }

    val isRunning: Boolean get() = manager.isRunning

    fun start(settings: TorrentSettings, uploadLimitBps: Int, lsdAllowed: Boolean) {
        if (manager.isRunning) return
        stateDir.mkdirs()
        val pack = settingsPack(settings, uploadLimitBps, lsdAllowed, includeListen = true)
        val stateFile = File(stateDir, SESSION_STATE)
        val params = try {
            if (stateFile.isFile) SessionParams(stateFile.readBytes()).also { it.setSettings(pack) } else SessionParams(pack)
        } catch (_: Exception) {
            SessionParams(pack)
        }
        manager.addListener(listener)
        manager.start(params)
    }

    /** Saves resume data of every torrent (waits up to [timeoutMs]), the session state, then stops. */
    fun stop(handles: List<TorrentHandle>, timeoutMs: Long = 4_000) {
        if (!manager.isRunning) return
        handles.forEach { requestResume(it) }
        val deadline = System.currentTimeMillis() + timeoutMs
        while (pendingResume.get() > 0 && System.currentTimeMillis() < deadline) {
            Thread.sleep(50)
        }
        try {
            manager.saveState()?.let { bytes -> atomicWrite(File(stateDir, SESSION_STATE), bytes) }
        } catch (_: Exception) {
            // state is a cache (DHT nodes)
        }
        manager.removeListener(listener)
        manager.stop()
        pendingResume.set(0)
        lastListenPort = null
    }

    fun applySettings(settings: TorrentSettings, uploadLimitBps: Int, lsdAllowed: Boolean) {
        if (!manager.isRunning) return
        val includeListen = lastListenPort != settings.listenPort
        manager.applySettings(settingsPack(settings, uploadLimitBps, lsdAllowed, includeListen))
    }

    fun find(id: String): TorrentHandle? = try {
        manager.find(Sha1Hash.parseHex(id))?.takeIf { it.isValid }
    } catch (_: Exception) {
        null
    }

    /** Synchronous add (returns the handle, or the existing one for a duplicate). Throws [IllegalStateException] on error. */
    fun add(params: AddTorrentParams): TorrentHandle {
        val ec = ErrorCode(error_code())
        val handle = SessionHandle(manager.swig()).addTorrent(params, ec)
        if (ec.isError || !handle.isValid) throw IllegalStateException(ec.message ?: "add failed")
        return handle
    }

    fun remove(handle: TorrentHandle, deleteFiles: Boolean) {
        if (!handle.isValid) return
        if (deleteFiles) manager.remove(handle, SessionHandle.DELETE_FILES) else manager.remove(handle, remove_flags_t())
    }

    fun requestResume(handle: TorrentHandle) {
        if (!handle.isValid) return
        try {
            pendingResume.incrementAndGet()
            handle.saveResumeData(TorrentHandle.SAVE_INFO_DICT)
        } catch (_: Exception) {
            pendingResume.decrementAndGet()
        }
    }

    fun resumeFile(id: String): File = File(stateDir, "$id.resume")

    /** Resume data written by a previous run (null when missing or unreadable). */
    fun readResume(id: String): AddTorrentParams? {
        val file = resumeFile(id)
        if (!file.isFile) return null
        return try {
            val ec = error_code()
            val params = libtorrent.read_resume_data_ex(Vectors.bytes2byte_vector(file.readBytes()), ec)
            if (ec.value() != 0) null else AddTorrentParams(params)
        } catch (_: Exception) {
            null
        }
    }

    private fun writeResume(alert: SaveResumeDataAlert) {
        try {
            val params = alert.params()
            val id = alert.handle().infoHash().toHex()
            atomicWrite(resumeFile(id), AddTorrentParams.writeResumeDataBuf(params))
        } catch (_: Exception) {
            // keep the previous resume file
        }
    }

    private fun settingsPack(settings: TorrentSettings, uploadLimitBps: Int, lsdAllowed: Boolean, includeListen: Boolean): SettingsPack {
        val s = settings.sanitized()
        val pack = SettingsPack()
        pack.downloadRateLimit(s.downloadLimitKib * 1024)
        pack.uploadRateLimit(uploadLimitBps)
        pack.connectionsLimit((s.maxPeers * 4).coerceIn(MIN_CONNECTIONS, MAX_CONNECTIONS))
        pack.setEnableDht(s.dht)
        pack.setEnableLsd(s.lsd && lsdAllowed)
        pack.setBoolean(settings_pack.bool_types.enable_upnp.swigValue(), s.upnp)
        pack.setBoolean(settings_pack.bool_types.enable_natpmp.swigValue(), s.upnp)
        if (includeListen) {
            val port = s.listenPort
            pack.listenInterfaces("0.0.0.0:$port,[::]:$port")
            lastListenPort = port
        }
        return pack
    }

    private fun atomicWrite(target: File, bytes: ByteArray) {
        target.parentFile?.mkdirs()
        val tmp = File(target.parentFile, target.name + ".tmp")
        tmp.writeBytes(bytes)
        if (!tmp.renameTo(target)) {
            target.delete()
            tmp.renameTo(target)
        }
    }

    companion object {
        private const val SESSION_STATE = "session.dat"
        private const val MIN_CONNECTIONS = 40
        private const val MAX_CONNECTIONS = 2_000

        private val LISTENED = listOf(
            AlertType.METADATA_RECEIVED,
            AlertType.SAVE_RESUME_DATA,
            AlertType.SAVE_RESUME_DATA_FAILED,
            AlertType.TORRENT_FINISHED,
            AlertType.FILE_COMPLETED,
            AlertType.TORRENT_ERROR,
            AlertType.FILE_ERROR,
            AlertType.STATE_CHANGED,
            AlertType.PIECE_FINISHED,
            AlertType.TORRENT_REMOVED,
        )

        /** Flags for every torrent we add: never auto-managed (the run policy decides), optionally without PEX. */
        fun addFlags(base: torrent_flags_t, paused: Boolean, pex: Boolean, dontDownloadByDefault: Boolean): torrent_flags_t {
            var flags = base.and_(TorrentFlags.AUTO_MANAGED.inv())
            flags = if (paused) flags.or_(TorrentFlags.PAUSED) else flags.and_(TorrentFlags.PAUSED.inv())
            if (!pex) flags = flags.or_(TorrentFlags.DISABLE_PEX)
            if (dontDownloadByDefault) flags = flags.or_(TorrentFlags.DEFAULT_DONT_DOWNLOAD)
            return flags
        }
    }
}
