// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SocialRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.bind
import org.koin.dsl.module
import java.io.File

/**
 * Koin bindings of the AniList social layer (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires `androidContext()`,
 * dataModule (ListsRepository, SettingsRepository), networkModule (OkHttpClient). [SocialScheduler] (eager) schedules / cancels
 * [SocialNotificationsWorker] following `AppSettings.socialNotifications` and the linked AniList account.
 */
val socialModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        SocialRepositoryImpl(
            client = get(),
            lists = get(),
            cacheDir = File(androidContext().cacheDir, SocialRepositoryImpl.CACHE_DIR),
            notificationsEnabled = { settings.settings.value.socialNotifications },
            texts = AndroidSocialTexts(androidContext()),
        )
    } bind AppDataWiper::class
    single<SocialRepository> { get<SocialRepositoryImpl>() }
    single<SocialActivityLookup> { get<SocialRepositoryImpl>() }
    single(createdAtStart = true) { SocialScheduler(androidContext(), get(), get()) }
}
