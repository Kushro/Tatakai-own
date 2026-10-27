package app.tatakai.next.feature.link

import android.os.Build
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val linkFeatureModule = module {
    viewModel { LinkViewModel(link = get(), deviceName = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android") }
}
