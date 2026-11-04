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
            settings = get(),
            // Bound by other workers of the android wave (torrentModule: f, dataModule: a); getOrNull keeps Details usable without them.
            torrents = getOrNull(),
            stats = getOrNull(),
        )
    }
}
