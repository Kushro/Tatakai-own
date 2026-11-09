package app.tatakai.next.feature.details

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** Parameters: (kind: MediaKind, ref: MediaRef) for Details; (characterId: Int) for the character page. */
val detailsModule = module {
    // parity wave A (15 §5 o.2, worker o): 18+ gate decisions and the episode view mode, private to this module.
    single<DetailsPrefs> { SharedPrefsDetailsPrefs(androidContext()) }
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
            prefs = get(),
        )
    }
    viewModel { params ->
        CharacterViewModel(
            characterId = params.get<Int>(),
            metadata = get(),
            // ListsRepository is bound by worker q in dataModule; without it the favorite heart is hidden.
            lists = getOrNull(),
        )
    }
}
