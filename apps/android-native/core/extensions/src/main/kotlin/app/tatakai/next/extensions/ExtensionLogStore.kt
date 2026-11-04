// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.ExtensionLogEntry
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.util.concurrent.ConcurrentHashMap

/**
 * In-memory, per-extension log (desktop `LogBuf`): newest last, at most [capacity] lines, each message at most [MAX_MESSAGE_CHARS].
 * `op` is one of install | fetch | stream | torrent | browser | compat | console | source | home. Lines are diagnostics (technical, English,
 * like desktop) and never carry request bodies, cookies or headers.
 */
class ExtensionLogStore(
    private val capacity: Int = DEFAULT_CAPACITY,
    private val clock: () -> Long = System::currentTimeMillis,
) {
    private val logs = ConcurrentHashMap<String, MutableStateFlow<List<ExtensionLogEntry>>>()

    private fun state(extId: String) = logs.getOrPut(extId) { MutableStateFlow(emptyList()) }

    fun append(extId: String, level: ExtLogLevel, op: String, message: String) {
        val text = if (message.length > MAX_MESSAGE_CHARS) message.take(MAX_MESSAGE_CHARS) + "..." else message
        val entry = ExtensionLogEntry(level, op, text, clock())
        state(extId).update { cur -> (if (cur.size >= capacity) cur.drop(cur.size - capacity + 1) else cur) + entry }
    }

    fun ok(extId: String, op: String, message: String) = append(extId, ExtLogLevel.OK, op, message)

    fun warn(extId: String, op: String, message: String) = append(extId, ExtLogLevel.WARN, op, message)

    fun err(extId: String, op: String, message: String) = append(extId, ExtLogLevel.ERR, op, message)

    fun flow(extId: String): StateFlow<List<ExtensionLogEntry>> = state(extId).asStateFlow()

    fun clear(extId: String) {
        logs.remove(extId)
    }

    companion object {
        const val DEFAULT_CAPACITY = 200
        const val MAX_MESSAGE_CHARS = 500
    }
}
