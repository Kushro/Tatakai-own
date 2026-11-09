// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.outlined.AccountCircle
import androidx.compose.material.icons.outlined.ChatBubbleOutline
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Media
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialUser
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage

// Parity wave B (16 §3.7, worker t): shared pieces of the social screens.

/** "Conecta AniList" (no linked account): the button opens Settings › Listas y cuentas. */
@Composable
internal fun ConnectAniList(onOpenAccounts: (() -> Unit)?, modifier: Modifier = Modifier) {
    EmptyState(
        icon = Icons.Outlined.AccountCircle,
        title = stringResource(R.string.social_connect_title),
        message = stringResource(R.string.social_connect_message),
        actionLabel = onOpenAccounts?.let { stringResource(R.string.social_connect_action) },
        onAction = onOpenAccounts,
        modifier = modifier,
    )
}

/** Full-screen error, or "Conecta AniList" when the error is the missing account. */
@Composable
internal fun SocialError(error: AppError, onRetry: () -> Unit, onOpenAccounts: (() -> Unit)?, modifier: Modifier = Modifier) {
    if (error.isNotLinked()) {
        ConnectAniList(onOpenAccounts, modifier)
    } else {
        ErrorState(
            message = error.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = onRetry,
            modifier = modifier,
            title = stringResource(R.string.social_error_title),
        )
    }
}

@Composable
internal fun DelayedLoading(modifier: Modifier = Modifier.fillMaxWidth().height(200.dp)) {
    if (rememberAfterDelay()) LoadingState(modifier)
}

/** "Cargar más" / inline error of a later page. */
@Composable
internal fun <T> PagedFooter(paged: Paged<T>, onMore: () -> Unit, onRetry: () -> Unit) {
    when {
        paged.loading && paged.items.isNotEmpty() -> DelayedLoading(Modifier.fillMaxWidth().height(64.dp))
        paged.error != null && paged.items.isNotEmpty() -> ErrorState(
            message = paged.error.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = onRetry,
            compact = true,
        )
        paged.hasNext && paged.items.isNotEmpty() -> Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.Center) {
            OutlinedButton(onClick = onMore) { Text(stringResource(R.string.social_load_more)) }
        }
    }
}

@Composable
internal fun Avatar(url: String?, size: Dp, modifier: Modifier = Modifier, contentDescription: String? = null) {
    Box(
        modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        if (url.isNullOrBlank()) {
            Icon(Icons.Outlined.Person, contentDescription = contentDescription, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            AsyncImage(model = url, contentDescription = contentDescription, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        }
    }
}

@Composable
internal fun UserRow(user: SocialUser, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Avatar(user.avatar, 40.dp)
        Column(Modifier.weight(1f)) {
            Text(user.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val rel = when {
                user.isFollowing && user.isFollower -> stringResource(R.string.social_mutual)
                user.isFollower -> stringResource(R.string.social_follows_you)
                user.isFollowing -> stringResource(R.string.social_following_state)
                else -> null
            }
            rel?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

/** The localised line of a list activity ("Vio el episodio 5 de Frieren"); AniList's own words for a status it does not list. */
@Composable
internal fun listActivityLine(a: SocialActivity.ListActivity): String {
    val title = a.media.title
    val p = a.progress
    return when (a.status.lowercase()) {
        "watched episode" -> if (p != null) stringResource(R.string.social_act_watched, p, title) else stringResource(R.string.social_act_watching, title)
        "rewatched episode" -> if (p != null) stringResource(R.string.social_act_rewatched, p, title) else stringResource(R.string.social_act_watching, title)
        "read chapter" -> if (p != null) stringResource(R.string.social_act_read, p, title) else stringResource(R.string.social_act_reading, title)
        "reread chapter" -> if (p != null) stringResource(R.string.social_act_reread, p, title) else stringResource(R.string.social_act_reading, title)
        "plans to watch" -> stringResource(R.string.social_act_plans_watch, title)
        "plans to read" -> stringResource(R.string.social_act_plans_read, title)
        "completed" -> stringResource(R.string.social_act_completed, title)
        "rewatched" -> stringResource(R.string.social_act_completed_again, title)
        "reread" -> stringResource(R.string.social_act_completed_again, title)
        "paused watching", "paused reading" -> stringResource(R.string.social_act_paused, title)
        "dropped" -> stringResource(R.string.social_act_dropped, title)
        else -> listOfNotNull(a.status.takeIf { it.isNotBlank() }, p, title).joinToString(" ")
    }
}

@Composable
internal fun ActivityCard(
    activity: SocialActivity,
    onOpenUser: (String) -> Unit,
    onOpenMedia: (Media) -> Unit,
    onOpenThread: ((Int) -> Unit)?,
    onLike: () -> Unit,
    expanded: Boolean = false,
) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.fillMaxWidth().then(if (onOpenThread != null) Modifier.clickable { onOpenThread(activity.id) } else Modifier),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Avatar(activity.user.avatar, 36.dp, Modifier.clickable { onOpenUser(activity.user.name) })
                Column(Modifier.weight(1f)) {
                    val who = (activity as? SocialActivity.TextActivity)?.recipient?.let { r ->
                        stringResource(R.string.social_message_to, activity.user.name, r.name)
                    } ?: activity.user.name
                    Text(who, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(relativeTime(activity.createdAtSec * 1000), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            when (activity) {
                is SocialActivity.ListActivity -> Row(
                    Modifier.fillMaxWidth().clickable { onOpenMedia(activity.media) },
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    MediaCover(activity.media.cover, Modifier.width(44.dp), contentDescription = null)
                    Text(listActivityLine(activity), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                }
                is SocialActivity.TextActivity -> Text(
                    activity.text,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = if (expanded) Int.MAX_VALUE else 8,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                val likeLabel = stringResource(if (activity.liked) R.string.social_unlike else R.string.social_like)
                TextButton(onClick = onLike, modifier = Modifier.semantics { contentDescription = likeLabel }) {
                    Icon(
                        if (activity.liked) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder,
                        contentDescription = null,
                        tint = if (activity.liked) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.size(18.dp),
                    )
                    Text(activity.likes.toString(), modifier = Modifier.padding(start = 6.dp))
                }
                if (onOpenThread != null) {
                    TextButton(onClick = { onOpenThread(activity.id) }) {
                        Icon(Icons.Outlined.ChatBubbleOutline, contentDescription = null, modifier = Modifier.size(18.dp))
                        Text(stringResource(R.string.social_replies_count, activity.replyCount), modifier = Modifier.padding(start = 6.dp))
                    }
                }
            }
        }
    }
}

/** Opens [url] in the browser / AniList app (`ACTION_VIEW`); nothing happens when no app can. */
internal fun openUrl(context: Context, url: String) {
    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) }
}

internal const val ANILIST_SETTINGS_URL = "https://anilist.co/settings"

internal fun reviewEditorUrl(anilistId: Int): String = "https://anilist.co/review/editor/$anilistId"
