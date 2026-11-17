// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.framegen.FgController
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.player.MPV_SESSION_FACTORY
import app.tatakai.next.player.PlayerSessionFactory
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.core.qualifier.named
import org.koin.dsl.module

/**
 * Koin bindings of the mpv engine (name FIXED: :app lists it). The libmpv [PlayerSessionFactory] is bound with the [MPV_SESSION_FACTORY]
 * qualifier; feature/player picks it when `AppSettings.playerEngine == MPV`. The shader preset comes from `AppSettings.playerPreset`.
 */
val mpvPlayerModule: Module = module {
    single<PlayerSessionFactory>(named(MPV_SESSION_FACTORY)) {
        val settings = getOrNull<SettingsRepository>()
        // 0.4.0 (worker B): the phone frame generation controller (frameGenModule of :core:framegen), when bound.
        MpvSessionFactory(androidContext(), { settings?.settings?.value?.playerPreset ?: "off" }, getOrNull<FgController>())
    }
}
