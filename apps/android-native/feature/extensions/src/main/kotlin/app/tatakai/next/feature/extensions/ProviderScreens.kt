// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.state.WaveBPlaceholder

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker r). Signatures FINAL; bodies: r.

/**
 * `Route.Provider(extensionId, sourceId)` (checklist ext.custom-providers.home / .list): the provider page of one custom source
 * (`ExtensionRepository.providerHome` rows + `providerSearch`); items → [onOpenItem]. The list of providers lives in ExtensionsScreen
 * (section "Proveedores" fed by `customSources()`), which navigates here with `onOpenProvider`.
 */
@Composable
fun ProviderScreen(
    extensionId: String,
    sourceId: String,
    onBack: () -> Unit,
    onOpenItem: (itemId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.extensions_provider_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.ProviderItem(extensionId, sourceId, itemId)` (checklist ext.custom-providers.info / .watch / .read / .bookmark / .continue):
 * `providerInfo` → cabecera, marcador ("Guardar" / "Completado": `setProviderBookmark`), entradas con filtro y orden; `watch` → [onPlay]
 * with `PlayerArgs(media = item.match ?: MediaRef(), title, episode = entry.number, providerRef = ...)` — the player resolves
 * `providerWatch` when `PlayerArgs.providerEntry` is set (§3.4); `read` → [onOpenReader] (`MangaReaderArgs` with the extension
 * coordinates, `mediaId = "ext:<ext>:<source>:<item>"`); a `match` with AniList id → [onOpenDetails].
 */
@Composable
fun ProviderItemScreen(
    extensionId: String,
    sourceId: String,
    itemId: String,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenDetails: (anilistId: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.extensions_provider_title), modifier = modifier, onBack = onBack)
}

/**
 * Settings › Reproductor › Fuentes, mounted by the contract after the source rows of `PlayerSection` (checklist
 * sources.embed-sniffer.timeout-setting, sources.embed-sniffer.ranking): switch "Fuentes embed (sniffer)" (`SourcePrefs.allowEmbed`) and
 * slider / stepper of `SourcePrefs.embedTimeoutSecs` (5..60 s). Empty until r implements it.
 */
@Composable
fun SourcesSettingsExtras(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    // wave B (r): rows described above.
}
