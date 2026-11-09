// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.content.Context
import android.net.Uri
import android.util.Log
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.StremioAddonRepository
import okhttp3.OkHttpClient
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.bind
import org.koin.dsl.module
import java.io.File
import java.io.FileInputStream
import java.io.IOException

/** Koin bindings of the extension host. Requires `androidContext()`. */
val extensionsModule: Module = module {
    // android wave (12 §3.7 / §3.16): per-extension log, hidden-WebView browser runtime for bundles with permitted browserDomains.
    single { ExtensionLogStore() }
    single<BrowserRuntime> { WebViewBrowserRuntime(androidContext(), get()) }
    single<JsEngine> { QuickJsEngine(browserRuntime = get()) }
    // Deliberately a fresh client: nothing from the app's own network stack (auth headers, OAuth tokens) may reach extensions.
    single(named("extensions-http")) { OkHttpClient.Builder().build() }
    single {
        val client = get<OkHttpClient>(named("extensions-http"))
        val logs = get<ExtensionLogStore>()
        ExtensionHost(
            engine = get(),
            hostFor = { manifest ->
                OkHttpJsHost(
                    manifest.networkPolicy,
                    client,
                    onLog = { level, msg ->
                        if (level == "error" || level == "warn") {
                            Log.w("Ext:${manifest.id}", msg)
                            logs.append(manifest.id, if (level == "error") ExtLogLevel.ERR else ExtLogLevel.WARN, "console", msg)
                        }
                    },
                    onEvent = { level, op, msg -> logs.append(manifest.id, level, op, msg) },
                )
            },
        )
    }
    // parity wave B (16 §3.3 / §3.4, worker t): embed sniffer (hidden WebView), native Stremio addons (one more ExternalCandidateSource),
    // their reset wiper. `StremioCandidateSource` is bound BEFORE the repository so `getAll<ExternalCandidateSource>()` below sees it.
    single<EmbedSniffer> { WebViewEmbedSniffer(androidContext(), get()) }
    single { StremioAddonRepositoryImpl(get(named("extensions-http")), File(androidContext().filesDir, StremioAddonRepositoryImpl.DIR_NAME), get()) }
    single<StremioAddonRepository> { get<StremioAddonRepositoryImpl>() }
    single { StremioCandidateSource(repository = get(), client = get(named("extensions-http"))) } bind ExternalCandidateSource::class
    single(named("stremio-wiper")) { AppDataWiper { get<StremioAddonRepositoryImpl>().wipe() } } bind AppDataWiper::class
    single {
        val context: Context = androidContext()
        val root = File(context.filesDir, "extensions").also { it.mkdirs() }
        ExtensionRepositoryImpl(
            rootDir = root,
            openUri = { uri ->
                val parsed = Uri.parse(uri)
                when (parsed.scheme) {
                    "content" -> context.contentResolver.openInputStream(parsed) ?: throw IOException("cannot open")
                    "file", null -> FileInputStream(File(parsed.path ?: uri))
                    else -> throw IOException("unsupported uri scheme")
                }
            },
            host = get(),
            settings = get<SettingsRepository>(),
            stats = get<SourceStatsStore>(),
            logStore = get(),
            // docs/redesign/13 §3.10: linked PC extension servers (:core:extension-server binds ExternalCandidateSource) + Stremio addons.
            external = getAll<ExternalCandidateSource>(),
            // parity wave B (16 §3.4, worker t): URL installs / repositories over the extensions' own client, SECRET preferences,
            // embed pages of custom/watch through the sniffer, custom sources of the PC servers (ExternalCustomSources).
            http = get(named("extensions-http")),
            secrets = getOrNull<SecretStore>(),
            downloadDir = File(context.cacheDir, "extension-downloads"),
            embedSniffer = { getOrNull<EmbedSniffer>() },
            externalCustom = getAll<ExternalCustomSources>(),
        )
    }
    single<ExtensionRepository> { get<ExtensionRepositoryImpl>() }
    single(named("extension-catalog-wiper")) { AppDataWiper { get<ExtensionRepositoryImpl>().wipeCatalog() } } bind AppDataWiper::class
}
