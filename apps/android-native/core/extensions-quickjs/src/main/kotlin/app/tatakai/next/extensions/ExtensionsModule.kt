// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import android.content.Context
import android.net.Uri
import android.util.Log
import app.tatakai.next.model.ExtensionRepository
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
    single<JsEngine> { QuickJsEngine() }
    // Deliberately a fresh client: nothing from the app's own network stack (auth headers, Supabase interceptors) may reach extensions.
    single(named("extensions-http")) { OkHttpClient.Builder().build() }
    single {
        val client = get<OkHttpClient>(named("extensions-http"))
        ExtensionHost(
            engine = get(),
            hostFor = { manifest ->
                OkHttpJsHost(manifest.networkPolicy, client) { level, msg ->
                    if (level == "error" || level == "warn") Log.w("Ext:${manifest.id}", msg)
                }
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
        )
    }
}
