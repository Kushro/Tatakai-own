package app.tatakai.next.feature.link

import android.os.Build
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val linkFeatureModule = module {
    viewModel {
        LinkViewModel(
            link = get(),
            deviceName = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android",
            // parity wave B (16 §3.11, worker r): "Continuar en la PC" + mDNS pairing
            history = getOrNull(),
            discovery = getOrNull(),
        )
    }
    // docs/redesign/13 §3.10: PC extension server connect screen
    viewModel { ExtensionServersViewModel(repository = get(), pending = get()) }
}
