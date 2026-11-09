// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.ProviderItem
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import org.koin.compose.koinInject

// Parity wave B closure (16 §3.4, worker t; checklist ext.custom-providers.continue): the "Continue watching" card of a provider title
// (`HistoryEntry.animeId = "ext:<extension>:<source>:<item>"`) resumes the episode in the player, like an AniList title, instead of only
// opening the provider page. History rows do not keep the provider entry id, so the title page is read once to map the episode back to
// its entry; when that fails (extension off, title gone, offline) the card falls back to the provider page, which shows its own error.

/** Coordinates of a provider title in a history id; extension ids may contain `:` (`server:<id>/<ext>`), so source and item come from the end. */
fun parseProviderHistoryId(animeId: String): Triple<String, String, String>? {
    if (!animeId.startsWith("ext:")) return null
    val rest = animeId.removePrefix("ext:")
    val item = rest.substringAfterLast(':', "")
    val head = rest.substringBeforeLast(':', "")
    val source = head.substringAfterLast(':', "")
    val ext = head.substringBeforeLast(':', "")
    if (ext.isEmpty() || source.isEmpty() || item.isEmpty()) return null
    return Triple(ext, source, item)
}

/**
 * Player arguments that resume [entry] of provider title [item]: the same episode at its position while it is unfinished, the next
 * entry once it was watched to the end. Null when the episode is no longer listed or the finished one was the last.
 */
fun providerResumeArgs(extensionId: String, sourceId: String, item: ProviderItem, entry: HistoryEntry): PlayerArgs? {
    val progress = mapOf(entry.episode to entry)
    val target = continueTarget(item.entries, progress) ?: return null
    return providerPlayerArgs(extensionId, sourceId, item, target, progress)
}

/** Resolves a provider history row to [PlayerArgs] with `ExtensionRepository.providerInfo`. */
class ProviderResumer(private val repo: ExtensionRepository, private val timeoutMs: Long = TIMEOUT_MS) {
    suspend fun resume(entry: HistoryEntry): PlayerArgs? {
        val (ext, source, item) = parseProviderHistoryId(entry.animeId) ?: return null
        val info: ProviderItem = (
            try {
                withTimeoutOrNull(timeoutMs) { repo.providerInfo(ext, source, item) }
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                null
            }
            ) ?: return null
        return providerResumeArgs(ext, source, info, entry)
    }

    companion object {
        const val TIMEOUT_MS = 15_000L
    }
}

/**
 * Click handler of a provider title's "Continue watching" card: plays the resumed episode with [onPlay], or opens the provider page with
 * [onOpenItem] when it cannot be resolved. Shows a small progress dialog while the page is read (dismissing it cancels).
 */
@Composable
fun rememberProviderResume(
    onPlay: (PlayerArgs) -> Unit,
    onOpenItem: (extensionId: String, sourceId: String, itemId: String) -> Unit,
): (HistoryEntry) -> Unit {
    val resumer = koinInject<ProviderResumer>()
    val scope = rememberCoroutineScope()
    val play by rememberUpdatedState(onPlay)
    val open by rememberUpdatedState(onOpenItem)
    var job by remember { mutableStateOf<Job?>(null) }

    if (job != null) {
        Dialog(onDismissRequest = {
            job?.cancel()
            job = null
        }) {
            Surface(shape = MaterialTheme.shapes.extraLarge, color = MaterialTheme.colorScheme.surfaceContainerHigh) {
                Row(Modifier.padding(24.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    CircularProgressIndicator(Modifier.size(24.dp))
                    Text(stringResource(R.string.extensions_provider_resuming), style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
    }

    return remember(resumer, scope) {
        { entry: HistoryEntry ->
            val coords = parseProviderHistoryId(entry.animeId)
            if (coords != null && job == null) {
                // Lazy start: `job` is set before the body can run, so the dialog always closes when it ends.
                val launched = scope.launch(start = CoroutineStart.LAZY) {
                    val args = resumer.resume(entry)
                    job = null
                    if (args != null) play(args) else open(coords.first, coords.second, coords.third)
                }
                job = launched
                launched.start()
            }
        }
    }
}
