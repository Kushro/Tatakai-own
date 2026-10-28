package app.tatakai.next.supabase

import org.koin.core.module.Module
import org.koin.dsl.module

/** Koin property keys; `:app` sets them from BuildConfig (`startKoin { properties(mapOf(...)) }`). */
const val PROP_SUPABASE_URL = "TATAKAI_SUPABASE_URL"
const val PROP_SUPABASE_ANON_KEY = "TATAKAI_SUPABASE_ANON_KEY"

val supabaseModule: Module = module {
    single {
        SupabaseGateway(
            SupabaseConfig(
                url = getProperty(PROP_SUPABASE_URL, ""),
                anonKey = getProperty(PROP_SUPABASE_ANON_KEY, ""),
            ),
        )
    }
    single<SupabaseAuthSource> { SupabaseAuthSourceImpl(get()) }
    single<WatchHistoryRemote> { WatchHistoryRemoteImpl(get()) }
}
