// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.player.MPV_SESSION_FACTORY
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.QualityRequest
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.module

/**
 * Koin bindings of the mpv engine (name FIXED: :app lists it). The libmpv [PlayerSessionFactory] is bound with the [MPV_SESSION_FACTORY]
 * qualifier; feature/player picks it when `AppSettings.playerEngine == MPV`. 0.4.0: each session starts from the quality defaults of
 * Settings › Reproductor (`AppSettings.playerPreset` + `AppSettings.playerQuality`), read when the session is created.
 */
val mpvPlayerModule: Module = module {
    single<PlayerSessionFactory>(named(MPV_SESSION_FACTORY)) {
        val settings = getOrNull<SettingsRepository>()
        MpvSessionFactory(androidContext()) {
            val s = settings?.settings?.value ?: AppSettings()
            QualityRequest.from(s.playerPreset, s.playerQuality)
        }
    }
}
