package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.metadata.providers.AniZipProvider
import app.tatakai.next.metadata.providers.KitsuProvider
import app.tatakai.next.metadata.providers.MalProvider
import app.tatakai.next.metadata.providers.ShikimoriProvider
import app.tatakai.next.metadata.providers.TenraiProvider
import okhttp3.OkHttpClient
import org.koin.core.module.Module
import org.koin.dsl.module

/** Supplies the user's enabled provider ids in priority order (implemented in :core:data from AppSettings). */
fun interface ProviderPriority {
    fun current(): List<String>
}

/** Koin property name of the MAL client id (defined by :core:data as PROP_MAL_CLIENT_ID; repeated here so :core:metadata stays JVM-only). */
const val METADATA_PROP_MAL_CLIENT_ID: String = "TATAKAI_MAL_CLIENT_ID"

/** Requires an [OkHttpClient] (networkModule) and a [ProviderPriority] (dataModule). */
val metadataModule: Module = module {
    single { AniListProvider(get<OkHttpClient>()) }
    single { AniZipProvider(get<OkHttpClient>()) }
    single { KitsuProvider(get<OkHttpClient>()) }
    // parity wave B (16 §3.1, worker r, plan 14 §0.1 #13): MAL / Shikimori / Tenrai in the router (stubs without capabilities until r).
    single { MalProvider(get<OkHttpClient>(), clientId = { getPropertyOrNull<String>(METADATA_PROP_MAL_CLIENT_ID) }) }
    single { ShikimoriProvider(get<OkHttpClient>()) }
    single { TenraiProvider(get<OkHttpClient>()) }
    // parity wave A (15 §3.1, worker o): trace.moe image search.
    single { TraceMoeClient(get<OkHttpClient>()) }
    single {
        val priority = get<ProviderPriority>()
        MetadataRouter(
            providers = listOf<MetadataProvider>(
                get<AniListProvider>(), get<AniZipProvider>(), get<KitsuProvider>(),
                get<MalProvider>(), get<ShikimoriProvider>(), get<TenraiProvider>(),
            ),
            priority = { priority.current() },
            imageSearch = get<TraceMoeClient>(),
        )
    }
    single { TrendingFeed(get()) }
}
