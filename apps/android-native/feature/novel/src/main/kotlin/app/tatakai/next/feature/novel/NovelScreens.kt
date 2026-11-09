// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.Media
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.ui.state.WaveBPlaceholder

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.6, worker s). Signatures FINAL; bodies: s.

/**
 * `Route.Novels(query)` (checklist novel.route / novel.home / novel.search): hero, tendencias, géneros, "Seguir leyendo", actualizaciones
 * de las que sigues, búsqueda con filtros (género, estado, origen, orden). [onMediaClick] opens `Route.NovelDetails(anilistId)`.
 */
@Composable
fun NovelHomeScreen(
    onMediaClick: (Media) -> Unit,
    onBack: (() -> Unit)?,
    onOpenReader: (NovelReaderArgs) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
    initialQuery: String? = null,
) {
    WaveBPlaceholder(title = stringResource(R.string.novel_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.NovelDetails(anilistId)` (checklist novel.details / .check-sources / novel.chapters / novel.readlist / novel.download.chapters):
 * sinopsis, metadatos, botón de lista de lectura, fuentes por extensión con "Comprobar todas" (`NovelRepository.checkSources`), filtro por
 * traductor, capítulos con idioma y fecha → [onOpenReader], selección múltiple → `downloadChapters`.
 */
@Composable
fun NovelDetailsScreen(
    anilistId: Int,
    onBack: () -> Unit,
    onOpenReader: (NovelReaderArgs) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenGenre: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.novel_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.NovelReader(args)` (checklist novel.reader.* / novel.tts.*): texto con la tipografía de `NovelReaderSettings`, scroll continuo
 * entre capítulos, barra de progreso + porcentaje + número de capítulo, pantalla completa, mantener pantalla encendida, hoja de ajustes
 * (tipografía y flujo) sin salir, "Leer en voz alta" con `Narrator` (play / pause / stop / velocidad, párrafo resaltado), guardar posición
 * al salir (`saveProgress`). Offline with `args.offline` (`offlineText`). Shown without the nav bar (shell).
 */
@Composable
fun NovelReaderScreen(
    args: NovelReaderArgs,
    onBack: () -> Unit,
    onReplace: (NovelReaderArgs) -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.novel_reader_title), modifier = modifier, onBack = onBack)
}

/** Library › Novelas (`LibraryTab.NOVELS`, mounted by the contract; checklist novel.readlist). Same shape as `MangaLibraryTab`. */
@Composable
fun NovelLibraryTab(onOpenDetails: (anilistId: Int) -> Unit, onOpenAccounts: () -> Unit, modifier: Modifier = Modifier) {
    WaveBPlaceholder(title = stringResource(R.string.novel_title), modifier = modifier)
}

/**
 * Settings › Lector › Novelas (mounted by the contract right after `ReaderSettingsSection`; checklist novel.tts.settings /
 * novel.reader.typography / novel.reader.custom-font): `NovelReaderSettings` rows (fuente, TTF/OTF importado a `filesDir/fonts/novel/`,
 * tamaño, interlineado, espaciado, ancho, fondo, justificar, scroll continuo, barra, pantalla completa, pantalla encendida) + Narrador
 * (`TtsSettings`: motor, voz con "Probar voz", velocidad, endpoint / modelo; clave en `SecretStore` bajo `SecretKeys.ttsApiKey`, UI
 * "clave guardada" / "Quitar").
 */
@Composable
fun NovelReaderSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    // wave B (s): rows described above.
}
