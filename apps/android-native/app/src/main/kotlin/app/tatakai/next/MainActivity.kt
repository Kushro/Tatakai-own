package app.tatakai.next

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.core.content.IntentCompat
import androidx.core.splashscreen.SplashScreen.Companion.installSplashScreen
import app.tatakai.next.data.OAuthRedirect
import app.tatakai.next.data.PendingOAuthRedirect
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

    /**
     * parity wave A (docs/redesign/15 §3.3): `tatakai://oauth/<provider>` came back from the Custom Tab. The URI waits in
     * [PendingOAuthRedirect] for worker q's ListsRepository; the shell opens Settings › Listas y cuentas.
     */
    private val accountLinks = MutableStateFlow(false)
    private val pendingOAuth: PendingOAuthRedirect by inject()

    /** parity wave A (15 §3.1): an image shared to the app (`ACTION_SEND image/*`) opens the image search with it. */
    private val imageLinks = MutableStateFlow<String?>(null)

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
            offerOAuthRedirect(intent)
            offerSharedImage(intent)
        }
        addOnNewIntentListener { newIntent ->
            TorrentLink.from(newIntent)?.let { torrentLinks.value = it }
            offerServerLink(newIntent)
            offerOAuthRedirect(newIntent)
            offerSharedImage(newIntent)
        }
        setContent { TatakaiApp(torrentLinks, serverLinks, accountLinks, imageLinks) }
    }

    /** Only `VIEW tatakai://extensionserver/connect…` intents; the link is validated (and confirmed by the user) by the screen. */
    private fun offerServerLink(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val data = intent.data ?: return
        if (!data.scheme.equals(DeepLink.SCHEME, ignoreCase = true) || !data.host.equals(DeepLink.HOST, ignoreCase = true)) return
        pendingServerLink.offer(data.toString())
        serverLinks.value = true
    }

    /** Only `VIEW tatakai://oauth/<anilist|mal>…` intents (the OAuth redirect); the code / state are checked by the lists repository. */
    private fun offerOAuthRedirect(intent: Intent?) {
        if (intent?.action != Intent.ACTION_VIEW) return
        val data = intent.data?.toString() ?: return
        if (OAuthRedirect.providerOf(data) == null) return
        pendingOAuth.offer(data)
        accountLinks.value = true
    }

    /** Only `SEND image/*` intents with a stream URI (share sheet). */
    private fun offerSharedImage(intent: Intent?) {
        if (intent?.action != Intent.ACTION_SEND || intent.type?.startsWith("image/") != true) return
        val uri = IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java) ?: return
        imageLinks.value = uri.toString()
    }
}
