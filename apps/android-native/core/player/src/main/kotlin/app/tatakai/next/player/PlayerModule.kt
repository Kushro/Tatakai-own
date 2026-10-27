// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.Container
import kotlinx.coroutines.CoroutineScope
import okhttp3.OkHttpClient
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.module

/** Creates [PlaybackController]s. A new client per app: nothing from the app's network stack (auth interceptors) reaches CDNs. */
@UnstableApi
class PlaybackControllerFactory(private val context: Context, private val client: OkHttpClient) {
    fun create(
        sources: List<PlayableSource>,
        scope: CoroutineScope,
        listener: PlaybackListener,
        remote: Boolean = false,
        startMs: Long = 0,
    ): PlaybackController = PlaybackController(context, sources, client, scope, listener, remote, startMs)
}

/** Remote (Link) playback: the PC's HLS is fetched with the mTLS client of the pairing. */
object RemotePlayback {
    fun source(access: RemotePlaybackAccess, sessionId: String, label: String): PlayableSource? {
        val session = access.session(sessionId) ?: return null
        val client = access.playbackClient() ?: return null
        return PlayableSource(url = session.playbackUrl, headers = emptyMap(), container = Container.HLS, label = label, httpClient = client)
    }
}

@UnstableApi
val playerModule: Module = module {
    single(named("player-http")) { OkHttpClient.Builder().build() }
    single { PlaybackControllerFactory(androidContext(), get(named("player-http"))) }
}

/**
 * Integration hook: `:core:player` must not depend on `:core:data`, so the app binds
 * `single<HistoryFlush> { HistoryFlush { get<HistorySyncScheduler>().requestSync(immediate = true) } }`.
 * The player calls it on pause and when leaving; when unbound the coalesced background sync still runs.
 */
fun interface HistoryFlush {
    fun requestImmediate()
}
