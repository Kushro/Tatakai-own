package app.tatakai.next

import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import app.tatakai.next.extensionserver.DeepLink
import app.tatakai.next.extensionserver.PendingServerLink
import app.tatakai.next.nav.TatakaiApp
import app.tatakai.next.nav.TorrentLink
import kotlinx.coroutines.flow.MutableStateFlow
import org.koin.android.ext.android.inject

/**
 * Single activity (edge-to-edge). The SplashScreen API keeps the splash until the first frame; there is no fixed delay.
 * Per-app language below API 33 is applied here through [AppLocale.wrap]; API 33+ uses LocaleManager (see [AppLocale]).
 * The system bar icon style follows the in-app theme mode ([TatakaiApp] re-calls `enableEdgeToEdge` when it changes).
 */
class MainActivity : ComponentActivity() {
    /** Pending torrent notification deep link (consumed and cleared by the shell). */
    private val torrentLinks = MutableStateFlow<TorrentLink?>(null)

    /** `tatakai://extensionserver/connect` opened (docs/redesign/13 §3.10): the shell opens the connect screen, which asks first. */
    private val serverLinks = MutableStateFlow(false)
    private val pendingServerLink: PendingServerLink by inject()

    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(AppLocale.wrap(newBase))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Torrent notifications relaunch the (singleTask) activity with extras: cold start reads the launch intent once,
        // a running activity receives them through onNewIntent.
        if (savedInstanceState == null) {
            torrentLinks.value = TorrentLink.from(intent)
            offerServerLink(intent)
        }
        addOnNewIntentListener { newIntent ->
            TorrentLink.from(newIntent)?.let { torrentLinks.value = it }
            offerServerLink(newIntent)
        }
        setContent { TatakaiApp(torrentLinks, serverLinks) }
    }

    /** Only `VIEW tatakai://extensionserver/connect…` intents; the link is validated (and confirmed by the user) by the screen. */
    private fun offerServerLink(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val data = intent.data ?: return
        if (!data.scheme.equals(DeepLink.SCHEME, ignoreCase = true) || !data.host.equals(DeepLink.HOST, ignoreCase = true)) return
        pendingServerLink.offer(data.toString())
        serverLinks.value = true
    }
}
