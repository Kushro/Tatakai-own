package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.metadata.providers.AniZipProvider
import app.tatakai.next.metadata.providers.KitsuProvider
import okhttp3.OkHttpClient
import org.koin.core.module.Module
import org.koin.dsl.module

/** Supplies the user's enabled provider ids in priority order (implemented in :core:data from AppSettings). */
fun interface ProviderPriority {
    fun current(): List<String>
}

/** Requires an [OkHttpClient] (networkModule) and a [ProviderPriority] (dataModule). */
val metadataModule: Module = module {
    single { AniListProvider(get<OkHttpClient>()) }
    single { AniZipProvider(get<OkHttpClient>()) }
    single { KitsuProvider(get<OkHttpClient>()) }
    single {
        val priority = get<ProviderPriority>()
        MetadataRouter(
            providers = listOf<MetadataProvider>(get<AniListProvider>(), get<AniZipProvider>(), get<KitsuProvider>()),
            priority = { priority.current() },
        )
    }
}
