// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import android.os.Build
import app.tatakai.next.extensions.ExternalCandidateSource
import app.tatakai.next.extensions.ExternalCustomSources
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.bind
import org.koin.dsl.module
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * A `tatakai://extensionserver/connect?…` link opened from outside (QR scanned by the camera app, a browser) waiting for the connect
 * screen. The screen asks the user before pairing: opening a link never connects on its own.
 */
class PendingServerLink {
    private val _link = MutableStateFlow<String?>(null)
    val link: StateFlow<String?> = _link.asStateFlow()

    fun offer(raw: String) {
        _link.value = raw
    }

    /** Returns and clears the pending link. */
    fun consume(): String? = _link.value.also { _link.value = null }
}

/** Koin bindings of the extension server client (docs/redesign/13 §3.10). Requires `androidContext()` and the shared [OkHttpClient]. */
val extensionServerModule: Module = module {
    // Derived from the shared client (User-Agent) without its HTTP cache and WITHOUT redirects: a redirect could leave the guarded
    // origin. No call timeout, so /resolve can stream; ServerCandidateSource bounds it. Short connect timeout: a PC that is off fails fast.
    single(named(HTTP)) {
        get<OkHttpClient>().newBuilder()
            .cache(null)
            .followRedirects(false)
            .followSslRedirects(false)
            .retryOnConnectionFailure(false)
            .connectTimeout(5, TimeUnit.SECONDS)
            .readTimeout(30, TimeUnit.SECONDS)
            .writeTimeout(15, TimeUnit.SECONDS)
            .callTimeout(0, TimeUnit.SECONDS)
            .build()
    }
    single { ExtensionServerClient(get(named(HTTP))) }
    single { ExtensionServerStore(File(androidContext().noBackupFilesDir, STORE_FILE), ExtensionServerKey.cipher()) }
    single {
        val model = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android"
        ExtensionServerRepositoryImpl(store = get(), client = get(), deviceLabel = "Tatakai Next ($model)".take(100))
    }
    single<ExtensionServerRepository> { get<ExtensionServerRepositoryImpl>() }
    single { ServerCandidateSource(repository = get(), client = get()) } bind ExternalCandidateSource::class
    // parity wave B (16 §3.4, worker t; ext.ecosystems.via-server): the PC servers' custom sources as provider pages.
    single { ServerCustomSources(repository = get(), client = get()) } bind ExternalCustomSources::class
    single { PendingServerLink() }
}

private const val HTTP = "extension-server-http"
private const val STORE_FILE = "extension-servers.bin"
