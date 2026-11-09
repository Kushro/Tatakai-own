// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import app.tatakai.next.model.Media
import app.tatakai.next.model.TitleLanguage

// Parity wave A (15 §5 o.2, checklist details.title-language): preferred title language of Settings › Metadatos. Internal twin of the
// helper in :feature:details (the integrator decides whether it moves to :core:ui).

/** The title in [language] (`Media.titles` keys romaji / english / native), falling back to the provider's main title. */
internal fun Media.displayTitle(language: TitleLanguage): String =
    titles[language.key]?.takeIf { it.isNotBlank() } ?: title

/** A secondary line: romaji, else native, else english, whichever differs from the [shown] title. */
internal fun Media.alternativeTitle(shown: String): String? =
    listOf("romaji", "native", "english").firstNotNullOfOrNull { key -> titles[key]?.takeIf { it.isNotBlank() && it != shown } }
