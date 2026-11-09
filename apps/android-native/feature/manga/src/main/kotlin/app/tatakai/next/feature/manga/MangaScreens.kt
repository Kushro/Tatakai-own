// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.Media
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.ui.state.WaveBPlaceholder

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.5, worker s). Signatures FINAL (the shell, Library and
// Settings wire them); bodies: s (one file per screen is fine, keep these names and parameters).

/**
 * `Route.Manga(query)` (checklist manga.route / manga.home / manga.home.followed-updates / manga.search / manga.adult-gate): hero,
 * Tendencias, Populares, Mejor valorados, chips de género, "Seguir leyendo" (`MangaRepository.lastRead`), novedades de lo que lees, zona 18+
 * con puerta (`AppSettings.showAdult` + `matureContent`), barra de búsqueda con filtros (origen JP/KR/CN, estado, género, año, orden →
 * `MediaSearchQuery.origin` etc.). [initialQuery] comes from the Search type chip. [onMediaClick] opens `Route.Details(MANGA, id)`.
 * [onBack] null on wide windows (rail destination).
 */
@Composable
fun MangaHomeScreen(
    onMediaClick: (Media) -> Unit,
    onBack: (() -> Unit)?,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
    initialQuery: String? = null,
) {
    WaveBPlaceholder(title = stringResource(R.string.manga_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.Details(MediaKind.MANGA, anilistId)` (checklist manga.details / .anime-link / manga.chapters.* / manga.readlist /
 * manga.download.chapters): cabecera, botón de lista de lectura (`MangaRepository.setReadStatus`, labels `ListStatus.readLabelRes()`),
 * sinopsis + metadatos (`MangaDetails`), puntuaciones, tags, personajes y recomendaciones (`MetadataRepository`), árbol de la serie
 * (`relations`), enlace a la adaptación anime ([onOpenAnime]), capítulos por extensión (`chapterSources` / `chapters`) con orden, filtro,
 * agrupación por volumen, Refrescar y Vaciar caché, selección múltiple → `downloadChapters`. [onOpenReader] with `MangaReaderArgs`.
 */
@Composable
fun MangaDetailsScreen(
    anilistId: Int,
    onBack: () -> Unit,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenAnime: (anilistId: Int) -> Unit,
    onOpenManga: (Media) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenGenre: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.manga_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.MangaReader(args)` (checklist manga.reader.*): paginado / vertical / webtoon, doble página, LTR/RTL, ajuste, fondo, brillo, recorte,
 * carga ansiosa / perezosa + prefetch (70 % → siguiente capítulo), scroll infinito, carril de miniaturas, navegador de capítulos, auto-scroll,
 * zoom (pellizco, doble toque, teclas), pantalla completa, ocultar chrome, progreso, captura, cambio de extensión si fallan las páginas,
 * guardar progreso al salir (`MangaRepository.saveProgress`), atajos (`ReaderSettings.keymap`). Offline with `args.offline`
 * (`offlinePages`). [onReplace] = next / previous chapter (same route, new args). Shown without the nav bar (shell).
 */
@Composable
fun MangaReaderScreen(
    args: MangaReaderArgs,
    onBack: () -> Unit,
    onReplace: (MangaReaderArgs) -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.manga_reader_title), modifier = modifier, onBack = onBack)
}

/**
 * Library › Manga (`LibraryTab.MANGA`, mounted by the contract in LibraryScreen; checklist manga.readlist): segmented of `ListStatus`
 * (labels `readLabelRes()`), rows with progress "x / y capítulos", "Seguir leyendo"; empty state → [onOpenAccounts] when nothing local and
 * no AniList account. Local first; AniList progress when linked.
 */
@Composable
fun MangaLibraryTab(onOpenDetails: (anilistId: Int) -> Unit, onOpenAccounts: () -> Unit, modifier: Modifier = Modifier) {
    // wave B (s): implement; the placeholder below is the only allowed stub body.
    WaveBPlaceholder(title = stringResource(R.string.manga_title), modifier = modifier)
}

/**
 * Settings › Lector (`SettingsSection.READER`, mounted by the contract; checklist manga.settings.section / manga.reader.keybinds /
 * manga.title-language): every `ReaderSettings` field + `AppSettings.mangaTitleLanguage` + keymap editor. Same shape as `PlayerSettingsExtras`.
 * The novel rows (`NovelReaderSettingsSection` of :feature:novel) are mounted right after this one by the contract.
 */
@Composable
fun ReaderSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    // wave B (s): rows described above.
}
