package app.tatakai.next

import android.content.Context
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import app.tatakai.next.nav.TatakaiApp
import app.tatakai.next.nav.TorrentLink
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * Single activity (edge-to-edge). The SplashScreen API keeps the splash until the first frame; there is no fixed delay.
 * Per-app language below API 33 is applied here through [AppLocale.wrap]; API 33+ uses LocaleManager (see [AppLocale]).
 * The system bar icon style follows the in-app theme mode ([TatakaiApp] re-calls `enableEdgeToEdge` when it changes).
 */
class MainActivity : ComponentActivity() {
    /** Pending torrent notification deep link (consumed and cleared by the shell). */
    private val torrentLinks = MutableStateFlow<TorrentLink?>(null)

    override fun attachBaseContext(newBase: Context) {
        super.attachBaseContext(AppLocale.wrap(newBase))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        installSplashScreen()
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Torrent notifications relaunch the (singleTask) activity with extras: cold start reads the launch intent once,
        // a running activity receives them through onNewIntent.
        if (savedInstanceState == null) torrentLinks.value = TorrentLink.from(intent)
        addOnNewIntentListener { newIntent -> TorrentLink.from(newIntent)?.let { torrentLinks.value = it } }
        setContent { TatakaiApp(torrentLinks) }
    }
}
