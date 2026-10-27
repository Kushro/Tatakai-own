package app.tatakai.next.network

import okhttp3.Cache
import okhttp3.Interceptor
import okhttp3.OkHttpClient
import okhttp3.Response
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module
import java.io.File
import java.util.concurrent.TimeUnit

/** Koin property that overrides the version shown in the User-Agent (optional). */
const val PROP_APP_VERSION = "tatakai.appVersion"

/**
 * The single shared OkHttp client for third-party public APIs (AniList, ani.zip, Kitsu, images).
 *
 * It deliberately has NO authentication interceptor: the Supabase session lives in :core:supabase (its own Ktor
 * engine, restricted to the configured Supabase URL) and must never be attached to third-party hosts.
 */
object HttpClientFactory {
    const val CACHE_BYTES = 20L * 1024 * 1024

    fun create(cacheDir: File?, appVersion: String = "dev"): OkHttpClient =
        OkHttpClient.Builder()
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(20, TimeUnit.SECONDS)
            .writeTimeout(20, TimeUnit.SECONDS)
            .callTimeout(30, TimeUnit.SECONDS)
            .retryOnConnectionFailure(true)
            .addInterceptor(UserAgentInterceptor("Tatakai/$appVersion (Android)"))
            .apply { if (cacheDir != null) cache(Cache(File(cacheDir, "http"), CACHE_BYTES)) }
            .build()
}

class UserAgentInterceptor(private val userAgent: String) : Interceptor {
    override fun intercept(chain: Interceptor.Chain): Response {
        val req = chain.request()
        return if (req.header("User-Agent") == null) {
            chain.proceed(req.newBuilder().header("User-Agent", userAgent).build())
        } else {
            chain.proceed(req)
        }
    }
}

val networkModule: Module = module {
    single<OkHttpClient> {
        HttpClientFactory.create(
            cacheDir = androidContext().cacheDir,
            appVersion = getPropertyOrNull<String>(PROP_APP_VERSION) ?: "dev",
        )
    }
}
