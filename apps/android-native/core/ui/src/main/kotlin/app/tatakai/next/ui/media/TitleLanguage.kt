// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.media

import androidx.compose.runtime.compositionLocalOf
import app.tatakai.next.model.Media
import app.tatakai.next.model.TitleLanguage

// Parity wave A (checklist details.title-language): preferred title language of Settings › Metadatos, shared by every surface that
// shows a Media title (MediaCard in Home, Library, character appearances, Search, Details). Moved up from :feature:search and
// :feature:details, which kept internal twins.

/**
 * The user's `AppSettings.titleLanguage`, provided once by the app shell (`TatakaiApp`) so shared components such as [MediaCard]
 * follow it without every screen threading it through. Defaults to [TitleLanguage.ROMAJI], the settings default.
 */
val LocalTitleLanguage = compositionLocalOf { TitleLanguage.ROMAJI }

/** The title in [language] (`Media.titles` keys romaji / english / native), falling back to the provider's main title. */
fun Media.displayTitle(language: TitleLanguage): String = titles[language.key]?.takeIf { it.isNotBlank() } ?: title

/** A secondary line: romaji, else native, else english, whichever differs from the [shown] title. */
fun Media.alternativeTitle(shown: String): String? =
    listOf("romaji", "native", "english").firstNotNullOfOrNull { key -> titles[key]?.takeIf { it.isNotBlank() && it != shown } }
