// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.content.Context
import android.net.Uri
import android.util.Log
import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceStatsStore
import okhttp3.OkHttpClient
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
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
    single<ExtensionRepository> {
        val context: Context = androidContext()
        ExtensionRepositoryImpl(
            rootDir = File(context.filesDir, "extensions").also { it.mkdirs() },
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
            // docs/redesign/13 §3.10: linked PC extension servers (:core:extension-server binds ExternalCandidateSource).
            external = getAll<ExternalCandidateSource>(),
        )
    }
}
