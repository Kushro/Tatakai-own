package app.tatakai.next.feature.details

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** Parameters: (kind: MediaKind, ref: MediaRef). */
val detailsModule = module {
    viewModel { params ->
        DetailsViewModel(
            kind = params.get<MediaKind>(),
            ref = params.get<MediaRef>(),
            metadata = get(),
            history = get(),
            extensions = get(),
            link = get(),
        )
    }
}
