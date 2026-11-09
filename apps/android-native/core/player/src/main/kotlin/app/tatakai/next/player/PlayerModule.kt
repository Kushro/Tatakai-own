// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.Container
import app.tatakai.next.model.SkipMarksRepository
import kotlinx.coroutines.CoroutineScope
import okhttp3.OkHttpClient
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.module

/**
 * Creates engine sessions. Media3 [PlaybackController]s come from here; the mpv engine registers a [PlayerSessionFactory] with the
 * [MPV_SESSION_FACTORY] qualifier in `mpvPlayerModule` (:core:player-mpv). A new client per app: nothing from the app's network stack
 * (auth interceptors) reaches CDNs.
 */
@UnstableApi
class PlaybackControllerFactory(private val context: Context, private val client: OkHttpClient) : PlayerSessionFactory {
    override fun create(scope: CoroutineScope, listener: PlaybackListener, remote: Boolean): PlaybackController =
        PlaybackController(context, client, scope, listener, remote)
}

/** Creates one [PlayerSession] of an engine (main thread). */
fun interface PlayerSessionFactory {
    fun create(scope: CoroutineScope, listener: PlaybackListener, remote: Boolean): PlayerSession
}

/** Koin qualifier of the libmpv [PlayerSessionFactory] (bound by `mpvPlayerModule`). */
const val MPV_SESSION_FACTORY = "mpv-session-factory"

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
    // android wave (12 §3.2, §3.16): AniSkip intro / outro / recap marks for web AND torrent playback.
    single<SkipMarksRepository> { AniSkipRepository(get(named("player-http"))) }
    // parity wave A (15 §3.2, worker p): private player preferences, subtitle fetch / translation / fonts.
    single { PlayerPrefs(androidContext()) }
    single { SubtitleTools(androidContext(), get(named(PLAYER_HTTP))) }
}

/** Koin qualifier of the player's own OkHttp client (no app interceptors reach CDNs). */
const val PLAYER_HTTP = "player-http"

/**
 * Integration hook: `:core:player` must not depend on `:core:data`, so the app binds
 * `single<HistoryFlush> { HistoryFlush { get<HistorySyncScheduler>().requestSync(immediate = true) } }`.
 * The player calls it on pause and when leaving; when unbound the coalesced background sync still runs.
 */
fun interface HistoryFlush {
    fun requestImmediate()
}
