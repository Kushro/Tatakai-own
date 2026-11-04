// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.AppSnackbar
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.feature.torrents.vm.TorrentMessage
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.labels.labelRes
import app.tatakai.next.ui.state.userMessage
import kotlin.math.roundToInt
import kotlinx.coroutines.flow.Flow

// Shared pieces of the torrent client screens (11 §3): states, formatting, error texts, pills.

/** Mono text style for figures (01 §3: JetBrains Mono for numbers, desktop `--tk-font-mono`). */
@Composable
internal fun monoStyle(base: TextStyle = MaterialTheme.typography.bodyMedium): TextStyle = base.copy(fontFamily = TatakaiFonts.families.mono)

/** Loading indicator only after 150 ms (02 §5), nothing before. */
@Composable
internal fun DelayedLoading(modifier: Modifier = Modifier) {
    if (rememberAfterDelay(150)) LoadingState(modifier)
}

/** The three states of a [LoadState] with torrent-aware error texts. */
@Composable
internal fun <T> TorrentLoadState(
    state: LoadState<T>,
    onRetry: () -> Unit,
    modifier: Modifier = Modifier,
    errorTitle: String? = null,
    content: @Composable (T) -> Unit,
) {
    when (state) {
        LoadState.Loading -> DelayedLoading(modifier)
        is LoadState.Err -> ErrorState(
            message = torrentErrorMessage(state.cause),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = onRetry,
            modifier = modifier,
            title = errorTitle ?: stringResource(UiR.string.ui_error_title),
        )
        is LoadState.Ok -> content(state.value)
    }
}

/** Torrent error codes of TorrentRepositoryImpl (`torrent:<code>[:detail]`) -> readable text; anything else -> the shared mapping. */
@Composable
internal fun torrentErrorMessage(error: AppError): String {
    val msg = (error as? AppError.Unknown)?.msg.orEmpty()
    if (!msg.startsWith("torrent:")) return error.userMessage()
    val parts = msg.split(':')
    return when (parts.getOrNull(1)) {
        "missing" -> stringResource(R.string.torrent_shell_missing)
        "moved" -> stringResource(R.string.torrent_android_moved)
        "no_space" -> stringResource(R.string.torrent_android_no_space, formatBytes(parts.getOrNull(2)?.toLongOrNull() ?: 0))
        "no_file" -> stringResource(R.string.torrent_android_no_file)
        "background_off" -> stringResource(R.string.torrent_android_background_off)
        "incomplete" -> stringResource(R.string.torrent_android_incomplete)
        "export_unsupported" -> stringResource(R.string.torrent_android_export_unsupported)
        "invalid_source" -> stringResource(R.string.torrent_android_invalid_source)
        else -> stringResource(R.string.torrent_overview_action_error)
    }
}

/** Sends [TorrentMessage]s of a ViewModel to the global snackbar. */
@Composable
internal fun MessagesEffect(messages: Flow<TorrentMessage>, snackbar: AppSnackbar) {
    val context = LocalContext.current
    val texts = MessageTexts.current()
    LaunchedEffect(messages, snackbar) {
        messages.collect { m ->
            val text = when (m) {
                is TorrentMessage.Removed -> if (m.count == 1) texts.removedOne else context.resources.getQuantityString(R.plurals.torrent_library_removed, m.count, m.count)
                is TorrentMessage.Exported -> texts.exported
                is TorrentMessage.Saved -> texts.saved
                is TorrentMessage.Failed -> texts.failed
            }
            snackbar.show(text)
        }
    }
}

/** Pre-resolved snackbar strings (collected outside composition). */
internal data class MessageTexts(val removedOne: String, val exported: String, val saved: String, val failed: String) {
    companion object {
        @Composable
        fun current(): MessageTexts = MessageTexts(
            removedOne = stringResource(R.string.torrent_overview_removed),
            exported = stringResource(R.string.torrent_android_exported),
            saved = stringResource(UiR.string.ui_settings_saved),
            failed = stringResource(R.string.torrent_library_action_failed),
        )
    }
}

/** Tone of a torrent state pill (12 §3.9: downloading/seeding OK, paused NEUTRAL, error ERR, initializing PRIMARY). */
internal fun TorrentState.tone(): PillTone = when (this) {
    TorrentState.DOWNLOADING, TorrentState.SEEDING -> PillTone.OK
    TorrentState.PAUSED -> PillTone.NEUTRAL
    TorrentState.ERROR -> PillTone.ERR
    TorrentState.INITIALIZING -> PillTone.PRIMARY
}

@Composable
internal fun StatePill(state: TorrentState, modifier: Modifier = Modifier) {
    Pill(text = stringResource(state.labelRes()), modifier = modifier, tone = state.tone())
}

/** "42%" / "42 %" (locale-specific through `torrent_fmt_percent`). */
@Composable
internal fun percentText(fraction: Double): String =
    stringResource(R.string.torrent_fmt_percent, (fraction.coerceIn(0.0, 1.0) * 100).roundToInt())

/** Desktop duration format: `12 s`, `3 min 4 s`, `1 h 5 min`, `2 d 3 h`; null -> `—`. */
@Composable
internal fun durationText(secs: Long?): String {
    if (secs == null || secs < 0) return stringResource(R.string.torrent_fmt_none)
    val d = secs / 86_400
    val h = (secs % 86_400) / 3_600
    val m = (secs % 3_600) / 60
    val s = secs % 60
    return when {
        d > 0 -> stringResource(R.string.torrent_fmt_dur_dh, d, h)
        h > 0 -> stringResource(R.string.torrent_fmt_dur_hm, h, m)
        m > 0 -> stringResource(R.string.torrent_fmt_dur_ms, m, s)
        else -> stringResource(R.string.torrent_fmt_dur_s, s)
    }
}

/** `HistoryEntry.animeIdOf` inverse: "123" -> anilist 123, "mal:45" -> MAL 45. */
internal fun mediaRefOf(mediaId: String?): MediaRef {
    val id = mediaId?.trim().orEmpty()
    return when {
        id.startsWith("mal:") -> MediaRef(malId = id.removePrefix("mal:").toIntOrNull())
        else -> MediaRef(anilistId = id.toIntOrNull())
    }
}

/** Player arguments for a file of a library torrent (client "Play this file", Library "Play"). */
internal fun playArgsFor(item: TorrentLibraryItem?, torrentId: String, fileIndex: Int?, episode: Int?, fallbackTitle: String): PlayerArgs =
    PlayerArgs(
        media = mediaRefOf(item?.mediaId),
        title = item?.series ?: fallbackTitle,
        episode = episode ?: item?.episode ?: 1,
        torrentId = torrentId,
        fileIndex = fileIndex,
    )

internal fun magnetOf(id: String): String = "magnet:?xt=urn:btih:$id"

internal fun copyToClipboard(context: Context, label: String, text: String) {
    runCatching {
        context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText(label, text))
    }
}

internal fun shareText(context: Context, text: String, chooserTitle: String) {
    val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text)
    runCatching {
        context.startActivity(Intent.createChooser(send, chooserTitle).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
    }
}

/** Label + value cell of the Overview grid. */
@Composable
internal fun FigureCell(label: String, value: String, modifier: Modifier = Modifier) {
    Column(modifier.padding(vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Text(value, style = monoStyle(MaterialTheme.typography.bodyLarge), maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

/** Inline banner (error / info) colored by role. */
@Composable
internal fun Banner(text: String, modifier: Modifier = Modifier, error: Boolean = false) {
    Surface(
        modifier = modifier.fillMaxWidth(),
        shape = MaterialTheme.shapes.medium,
        color = if (error) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (error) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer,
    ) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}
