// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.NotificationsNone
import androidx.compose.material.icons.outlined.PersonAdd
import androidx.compose.material.icons.outlined.PersonRemove
import androidx.compose.material.icons.outlined.RateReview
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.Review
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialStats
import app.tatakai.next.model.SocialUser
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.7, worker t). Signatures FINAL. Every screen shows
// "Conecta AniList" (EmptyState → [onOpenAccounts]) when there is no linked AniList account (`SOCIAL_NOT_LINKED`).

/** Snackbar of the one-shot outcomes of a social view model. */
@Composable
private fun NoticesEffect(notices: kotlinx.coroutines.flow.Flow<SocialNotice>, onReplied: () -> Unit = {}, onPosted: () -> Unit = {}) {
    val snackbar = LocalAppSnackbar.current
    val res = LocalContext.current.resources
    val failed = stringResource(R.string.social_action_failed)
    LaunchedEffect(notices) {
        notices.collect { n ->
            when (n) {
                SocialNotice.Posted -> {
                    onPosted()
                    snackbar.show(res.getString(R.string.social_posted))
                }
                SocialNotice.Replied -> onReplied()
                is SocialNotice.Failed -> snackbar.show(if (n.error.isNotLinked()) res.getString(R.string.social_connect_title) else failed)
            }
        }
    }
}

/**
 * `Route.Profile(userName)` (checklist social.profile.page / .public / social.follow / social.follow.lists / social.activity.feed /
 * social.activity.interact): null = the viewer. Avatar, banner, name, about, followers / following (paginated lists), statistics
 * (`SocialStats`), favourites, Activity tab (own + of the people the viewer follows with `followingOnly`), "Seguir" / "Siguiendo"
 * (`toggleFollow`), status post (`postStatus`), "Editar perfil" → `https://anilist.co/settings` by `ACTION_VIEW`. [onBack] null on wide windows.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProfileScreen(
    userName: String?,
    onBack: (() -> Unit)?,
    onOpenUser: (userName: String) -> Unit,
    onOpenActivity: (activityId: Int) -> Unit,
    onOpenMedia: (Media) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: ProfileViewModel = koinViewModel(key = "profile-${userName ?: "@me"}", parameters = { parametersOf(userName) })
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var draft by rememberSaveable { mutableStateOf("") }
    NoticesEffect(vm.notices, onPosted = { draft = "" })
    val user = (state.user as? LoadState.Ok)?.value
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(user?.name ?: stringResource(R.string.social_profile_title), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                    }
                },
                actions = {
                    IconButton(onClick = vm::load) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.social_refresh)) }
                    if (user != null) {
                        IconButton(onClick = { openUrl(context, user.siteUrl) }) {
                            Icon(Icons.AutoMirrored.Outlined.OpenInNew, contentDescription = stringResource(R.string.social_open_anilist))
                        }
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            when (val s = state.user) {
                LoadState.Loading -> DelayedLoading(Modifier.fillMaxSize())
                is LoadState.Err -> SocialError(s.cause, onRetry = vm::load, onOpenAccounts = onOpenAccounts, modifier = Modifier.fillMaxSize())
                is LoadState.Ok -> ProfileContent(
                    user = s.value,
                    state = state,
                    vm = vm,
                    draft = draft,
                    onDraft = { draft = it },
                    onOpenUser = onOpenUser,
                    onOpenActivity = onOpenActivity,
                    onOpenMedia = onOpenMedia,
                    onEditProfile = { openUrl(context, ANILIST_SETTINGS_URL) },
                )
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ProfileContent(
    user: SocialUser,
    state: ProfileUiState,
    vm: ProfileViewModel,
    draft: String,
    onDraft: (String) -> Unit,
    onOpenUser: (String) -> Unit,
    onOpenActivity: (Int) -> Unit,
    onOpenMedia: (Media) -> Unit,
    onEditProfile: () -> Unit,
) {
    val activity by vm.activity.state.collectAsStateWithLifecycle()
    val followers by vm.followers.state.collectAsStateWithLifecycle()
    val following by vm.following.state.collectAsStateWithLifecycle()
    LazyColumn(
        modifier = Modifier.fillMaxSize().widthIn(max = 840.dp),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "header", contentType = "header") {
            ProfileHeader(user, followBusy = state.followBusy, onFollow = vm::toggleFollow, onEdit = onEditProfile, onTab = vm::selectTab)
        }
        item(key = "tabs", contentType = "tabs") {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                ProfileTab.entries.forEach { t ->
                    FilterChip(selected = state.tab == t, onClick = { vm.selectTab(t) }, label = { Text(stringResource(t.labelRes())) })
                }
            }
        }
        when (state.tab) {
            ProfileTab.ACTIVITY -> {
                if (user.isViewer) {
                    item(key = "compose", contentType = "compose") {
                        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedTextField(
                                    value = draft,
                                    onValueChange = onDraft,
                                    label = { Text(stringResource(R.string.social_status_hint)) },
                                    modifier = Modifier.weight(1f),
                                    maxLines = 4,
                                )
                                IconButton(onClick = { vm.postStatus(draft) }, enabled = draft.isNotBlank() && !state.posting) {
                                    Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.social_post))
                                }
                            }
                            FilterChip(
                                selected = state.followingOnly,
                                onClick = { vm.setFollowingOnly(!state.followingOnly) },
                                label = { Text(stringResource(R.string.social_following_only)) },
                            )
                        }
                    }
                }
                when {
                    activity.initialLoading -> item(key = "act-loading", contentType = "state") { DelayedLoading() }
                    activity.initialError != null -> item(key = "act-error", contentType = "state") {
                        SocialError(activity.initialError!!, onRetry = vm.activity::retry, onOpenAccounts = null)
                    }
                    activity.items.isEmpty() && !activity.loading -> item(key = "act-empty", contentType = "state") {
                        EmptyState(icon = Icons.Outlined.NotificationsNone, title = stringResource(R.string.social_activity_empty))
                    }
                    else -> items(activity.items, key = { "a:${it.id}" }, contentType = { "activity" }) { a ->
                        ActivityCard(a, onOpenUser = onOpenUser, onOpenMedia = onOpenMedia, onOpenThread = onOpenActivity, onLike = { vm.toggleLike(a.id) })
                    }
                }
                item(key = "act-footer", contentType = "footer") { PagedFooter(activity, onMore = vm.activity::loadMore, onRetry = vm.activity::retry) }
            }
            ProfileTab.STATS -> item(key = "stats", contentType = "stats") { StatsGrid(user.stats) }
            ProfileTab.FAVORITES -> item(key = "favs", contentType = "favs") { Favorites(user, onOpenMedia) }
            ProfileTab.FOLLOWERS, ProfileTab.FOLLOWING -> {
                val list = if (state.tab == ProfileTab.FOLLOWERS) followers else following
                val pager = if (state.tab == ProfileTab.FOLLOWERS) vm.followers else vm.following
                when {
                    list.initialLoading || (list.page == 0 && list.error == null) -> item(key = "f-loading", contentType = "state") { DelayedLoading() }
                    list.initialError != null -> item(key = "f-error", contentType = "state") {
                        SocialError(list.initialError!!, onRetry = pager::retry, onOpenAccounts = null)
                    }
                    list.items.isEmpty() -> item(key = "f-empty", contentType = "state") {
                        EmptyState(icon = Icons.Outlined.PersonAdd, title = stringResource(R.string.social_follow_empty))
                    }
                    else -> items(list.items, key = { "u:${state.tab}:${it.id}" }, contentType = { "user" }) { u -> UserRow(u, onClick = { onOpenUser(u.name) }) }
                }
                item(key = "f-footer", contentType = "footer") { PagedFooter(list, onMore = pager::loadMore, onRetry = pager::retry) }
            }
        }
    }
}

private fun ProfileTab.labelRes(): Int = when (this) {
    ProfileTab.ACTIVITY -> R.string.social_tab_activity
    ProfileTab.STATS -> R.string.social_tab_stats
    ProfileTab.FAVORITES -> R.string.social_tab_favorites
    ProfileTab.FOLLOWERS -> R.string.social_tab_followers
    ProfileTab.FOLLOWING -> R.string.social_tab_following
}

@Composable
private fun ProfileHeader(user: SocialUser, followBusy: Boolean, onFollow: () -> Unit, onEdit: () -> Unit, onTab: (ProfileTab) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Box(Modifier.fillMaxWidth()) {
            Box(
                Modifier.fillMaxWidth().aspectRatio(3.2f).clip(MaterialTheme.shapes.large).background(MaterialTheme.colorScheme.surfaceContainerHigh),
            ) {
                if (!user.banner.isNullOrBlank()) {
                    AsyncImage(model = user.banner, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                }
            }
            Box(Modifier.align(Alignment.BottomStart).offset(x = 16.dp, y = 32.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surface).padding(3.dp)) {
                Avatar(user.avatar, 80.dp, contentDescription = user.name)
            }
        }
        Row(Modifier.fillMaxWidth().padding(top = 28.dp), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(user.name, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.semantics { heading() })
                if (user.isFollower && !user.isViewer) Pill(stringResource(R.string.social_follows_you), tone = PillTone.NEUTRAL)
            }
            when {
                user.isViewer -> OutlinedButton(onClick = onEdit) {
                    Icon(Icons.Outlined.Edit, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.social_edit_profile), modifier = Modifier.padding(start = 8.dp))
                }
                user.isFollowing -> OutlinedButton(onClick = onFollow, enabled = !followBusy) {
                    Icon(Icons.Outlined.PersonRemove, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.social_following_button), modifier = Modifier.padding(start = 8.dp))
                }
                else -> Button(onClick = onFollow, enabled = !followBusy) {
                    Icon(Icons.Outlined.PersonAdd, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.social_follow), modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Text(
                stringResource(R.string.social_followers_count, user.followers),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.clickable { onTab(ProfileTab.FOLLOWERS) },
            )
            Text(
                stringResource(R.string.social_following_count, user.following),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.clickable { onTab(ProfileTab.FOLLOWING) },
            )
        }
        user.about?.let { Text(it, style = MaterialTheme.typography.bodyMedium, maxLines = 12, overflow = TextOverflow.Ellipsis) }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun StatsGrid(stats: SocialStats?) {
    if (stats == null) {
        Text(stringResource(R.string.social_stats_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
        return
    }
    val days = stats.minutesWatched / 60.0 / 24.0
    FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        StatTile(stringResource(R.string.social_stat_anime), stats.animeCount.toString())
        StatTile(stringResource(R.string.social_stat_episodes), stats.episodesWatched.toString())
        StatTile(stringResource(R.string.social_stat_days), String.format(java.util.Locale.getDefault(), "%.1f", days))
        StatTile(stringResource(R.string.social_stat_mean), stats.meanScore?.let { String.format(java.util.Locale.getDefault(), "%.1f", it) } ?: "—")
        StatTile(stringResource(R.string.social_stat_manga), stats.mangaCount.toString())
        StatTile(stringResource(R.string.social_stat_chapters), stats.chaptersRead.toString())
    }
}

@Composable
private fun StatTile(label: String, value: String) {
    Surface(shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.width(150.dp)) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(value, style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.primary)
            Text(label, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun Favorites(user: SocialUser, onOpenMedia: (Media) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
        if (user.favoriteAnime.isEmpty() && user.favoriteManga.isEmpty() && user.favoriteCharacters.isEmpty()) {
            Text(stringResource(R.string.social_favorites_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        FavoriteRow(stringResource(R.string.social_favorites_anime), user.favoriteAnime, onOpenMedia)
        FavoriteRow(stringResource(R.string.social_favorites_manga), user.favoriteManga, onOpenMedia)
        if (user.favoriteCharacters.isNotEmpty()) {
            Text(stringResource(R.string.social_favorites_characters), style = MaterialTheme.typography.titleSmall)
            LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(user.favoriteCharacters, key = { it.id }) { c ->
                    Column(Modifier.width(80.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Avatar(c.image, 64.dp, contentDescription = c.name)
                        Text(c.name, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}

@Composable
private fun FavoriteRow(title: String, list: List<Media>, onOpenMedia: (Media) -> Unit) {
    if (list.isEmpty()) return
    Text(title, style = MaterialTheme.typography.titleSmall)
    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        items(list, key = { it.ref.anilistId ?: it.title.hashCode() }) { m ->
            Column(Modifier.width(100.dp).clickable { onOpenMedia(m) }, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                MediaCover(m.cover, Modifier.fillMaxWidth(), contentDescription = m.title)
                Text(m.title, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}

/** `Route.ActivityThread(activityId)` (checklist social.activity.interact): the activity, likes (`toggleLike`), replies, "Responder" (`reply`). */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ActivityThreadScreen(
    activityId: Int,
    onBack: () -> Unit,
    onOpenUser: (userName: String) -> Unit,
    onOpenMedia: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: ActivityThreadViewModel = koinViewModel(key = "activity-$activityId", parameters = { parametersOf(activityId) })
    val state by vm.state.collectAsStateWithLifecycle()
    val replies by vm.replies.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    var draft by rememberSaveable { mutableStateOf("") }
    NoticesEffect(vm.notices, onReplied = { draft = "" })
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.social_activity_title)) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) } },
                actions = {
                    (state.activity as? LoadState.Ok)?.value?.let { a ->
                        IconButton(onClick = { openUrl(context, a.siteUrl) }) {
                            Icon(Icons.AutoMirrored.Outlined.OpenInNew, contentDescription = stringResource(R.string.social_open_anilist))
                        }
                    }
                },
            )
        },
        bottomBar = {
            if (state.activity is LoadState.Ok) {
                Surface(color = MaterialTheme.colorScheme.surfaceContainer) {
                    Row(Modifier.fillMaxWidth().imePadding().padding(horizontal = 12.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        OutlinedTextField(
                            value = draft,
                            onValueChange = { draft = it },
                            label = { Text(stringResource(R.string.social_reply_hint)) },
                            modifier = Modifier.weight(1f),
                            maxLines = 4,
                        )
                        IconButton(onClick = { vm.reply(draft) }, enabled = draft.isNotBlank() && !state.sending) {
                            Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.social_reply))
                        }
                    }
                }
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            when (val s = state.activity) {
                LoadState.Loading -> DelayedLoading(Modifier.fillMaxSize())
                is LoadState.Err -> SocialError(s.cause, onRetry = vm::load, onOpenAccounts = null, modifier = Modifier.fillMaxSize())
                is LoadState.Ok -> LazyColumn(
                    modifier = Modifier.fillMaxSize().widthIn(max = 840.dp),
                    contentPadding = PaddingValues(16.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    item(key = "activity") {
                        ActivityCard(s.value, onOpenUser = onOpenUser, onOpenMedia = onOpenMedia, onOpenThread = null, onLike = vm::toggleLike, expanded = true)
                    }
                    item(key = "replies-head") {
                        Text(stringResource(R.string.social_replies_count, s.value.replyCount), style = MaterialTheme.typography.titleSmall)
                    }
                    when {
                        replies.initialLoading -> item(key = "r-loading") { DelayedLoading(Modifier.fillMaxWidth().height(96.dp)) }
                        replies.initialError != null -> item(key = "r-error") { SocialError(replies.initialError!!, onRetry = vm.replies::retry, onOpenAccounts = null) }
                        replies.items.isEmpty() -> item(key = "r-empty") {
                            Text(stringResource(R.string.social_replies_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        else -> items(replies.items, key = { "r:${it.id}" }) { r ->
                            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                Avatar(r.user.avatar, 32.dp, Modifier.clickable { onOpenUser(r.user.name) })
                                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                                    Text("${r.user.name} · ${relativeTime(r.createdAtSec * 1000)}", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                                    Text(r.text, style = MaterialTheme.typography.bodyMedium)
                                }
                            }
                        }
                    }
                    item(key = "r-footer") { PagedFooter(replies, onMore = vm.replies::loadMore, onRetry = vm.replies::retry) }
                }
            }
        }
    }
}

/**
 * `Route.Notifications` (checklist social.notifications.list / .badge): `SocialRepository.notifications(markRead = true)` paginated;
 * AIRING → [onOpenMedia], FOLLOWING → [onOpenUser], likes / replies / mentions → [onOpenActivity]; pull to refresh; `refreshUnread` on resume.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NotificationsScreen(
    onBack: () -> Unit,
    onOpenMedia: (Media) -> Unit,
    onOpenActivity: (activityId: Int) -> Unit,
    onOpenUser: (userName: String) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: NotificationsViewModel = koinViewModel()
    val paged by vm.pager.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) { vm.onResume() }
    // Android 13+: the runtime permission for the 15-minute summary is asked here, never at app start (16 §0.12).
    var granted by remember {
        mutableStateOf(Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED)
    }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { ok -> granted = ok }
    val open: (SocialNotification) -> Unit = { n ->
        when (val t = notificationTarget(n)) {
            is NotificationTarget.OpenMedia -> onOpenMedia(t.media)
            is NotificationTarget.OpenActivity -> onOpenActivity(t.activityId)
            is NotificationTarget.OpenUser -> onOpenUser(t.userName)
            NotificationTarget.None -> Unit
        }
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.social_notifications_title)) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) } },
                actions = { IconButton(onClick = vm::refresh) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.social_refresh)) } },
            )
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = paged.loading && paged.items.isNotEmpty() && paged.page <= 1,
            onRefresh = vm::refresh,
            modifier = Modifier.fillMaxSize().padding(padding),
        ) {
            Box(Modifier.fillMaxSize(), contentAlignment = Alignment.TopCenter) {
                when {
                    paged.initialLoading || (paged.page == 0 && paged.error == null) -> DelayedLoading(Modifier.fillMaxSize())
                    paged.initialError != null -> SocialError(paged.initialError!!, onRetry = vm.pager::retry, onOpenAccounts = onOpenAccounts, modifier = Modifier.fillMaxSize())
                    else -> LazyColumn(
                        modifier = Modifier.fillMaxSize().widthIn(max = 840.dp),
                        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp),
                        verticalArrangement = Arrangement.spacedBy(4.dp),
                    ) {
                        if (!granted) {
                            item(key = "permission") {
                                Surface(shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.secondaryContainer, modifier = Modifier.fillMaxWidth()) {
                                    Row(Modifier.padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                                        Text(stringResource(R.string.social_permission_hint), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                                        TextButton(onClick = { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) permission.launch(Manifest.permission.POST_NOTIFICATIONS) }) {
                                            Text(stringResource(R.string.social_permission_allow))
                                        }
                                    }
                                }
                            }
                        }
                        if (paged.items.isEmpty()) {
                            item(key = "empty") { EmptyState(icon = Icons.Outlined.Notifications, title = stringResource(R.string.social_notifications_empty)) }
                        }
                        items(paged.items, key = { "n:${it.id}" }, contentType = { "notification" }) { n ->
                            NotificationRow(n, onClick = { open(n) })
                            HorizontalDivider()
                        }
                        item(key = "footer") { PagedFooter(paged, onMore = vm.pager::loadMore, onRetry = vm.pager::retry) }
                    }
                }
            }
        }
    }
}

@Composable
private fun NotificationRow(n: SocialNotification, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        val media = n.media
        if (media != null && n.user == null) {
            MediaCover(media.cover, Modifier.width(40.dp), contentDescription = null)
        } else {
            Avatar(n.user?.avatar, 40.dp)
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(n.text, style = MaterialTheme.typography.bodyMedium, maxLines = 3, overflow = TextOverflow.Ellipsis)
            Text(relativeTime(n.createdAtSec * 1000), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        if (n.unread) {
            val label = stringResource(R.string.social_unread)
            Box(Modifier.size(10.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary).semantics { this.contentDescription = label })
        }
    }
}

/**
 * Checklist details.reviews / details.comments-mount (stable id: "montaje de actividad / reseñas de AniList"): mounted by `WaveBSlots` of
 * Details (o's file, closed) at the end of the Information tab. Shows the AniList mean score, the reviews (`SocialRepository.reviews`:
 * author, score, summary, expand) with "Escribir reseña" → `https://anilist.co/review/editor/<id>` (ACTION_VIEW) and the viewer's own
 * activity on the title (`mediaActivity`). No own comments (plan 14 §0.1 #10). Nothing is drawn for a title without an AniList id.
 */
@Composable
fun AniListSocialSlot(media: Media, modifier: Modifier = Modifier) {
    val id = media.ref.anilistId ?: return
    val vm: SocialSlotViewModel = koinViewModel(key = "social-slot-$id", parameters = { parametersOf(id) })
    val reviews by vm.reviews.state.collectAsStateWithLifecycle()
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    Column(modifier.fillMaxWidth().padding(vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(stringResource(R.string.social_reviews_title), style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
                media.score?.let { Text(stringResource(R.string.social_mean_score, it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
            }
            TextButton(onClick = { openUrl(context, reviewEditorUrl(id)) }) {
                Icon(Icons.Outlined.RateReview, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.social_write_review), modifier = Modifier.padding(start = 8.dp))
            }
        }
        when {
            reviews.initialLoading -> DelayedLoading(Modifier.fillMaxWidth().height(96.dp))
            reviews.initialError != null -> app.tatakai.next.designsystem.components.ErrorState(
                message = reviews.initialError!!.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = vm.reviews::retry,
                compact = true,
            )
            reviews.items.isEmpty() -> Text(stringResource(R.string.social_reviews_empty), color = MaterialTheme.colorScheme.onSurfaceVariant)
            else -> reviews.items.forEach { r -> ReviewCard(r, onOpen = { openUrl(context, r.siteUrl) }) }
        }
        PagedFooter(reviews, onMore = vm.reviews::loadMore, onRetry = vm.reviews::retry)
        if (state.ownActivity.isNotEmpty()) {
            Text(stringResource(R.string.social_own_activity), style = MaterialTheme.typography.titleSmall)
            state.ownActivity.take(MAX_OWN_ACTIVITY).forEach { a ->
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    val line = when (a) {
                        is app.tatakai.next.model.SocialActivity.ListActivity -> listActivityLine(a)
                        is app.tatakai.next.model.SocialActivity.TextActivity -> a.text
                    }
                    Text(line, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(relativeTime(a.createdAtSec * 1000), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
    }
}

private const val MAX_OWN_ACTIVITY = 5

@Composable
private fun ReviewCard(r: Review, onOpen: () -> Unit) {
    var expanded by rememberSaveable(r.id) { mutableStateOf(false) }
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Avatar(r.user.avatar, 32.dp)
                Column(Modifier.weight(1f)) {
                    Text(r.user.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(R.string.social_review_helpful, r.rating, r.ratingAmount),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Pill(stringResource(R.string.social_review_score, r.score), tone = if (r.score >= 70) PillTone.OK else if (r.score >= 50) PillTone.WARN else PillTone.ERR)
            }
            Text(r.summary, style = MaterialTheme.typography.bodyMedium)
            if (expanded) Text(r.body, style = MaterialTheme.typography.bodySmall)
            Row {
                TextButton(onClick = { expanded = !expanded }) { Text(stringResource(if (expanded) R.string.social_review_collapse else R.string.social_review_expand)) }
                TextButton(onClick = onOpen) { Text(stringResource(R.string.social_open_anilist)) }
            }
        }
    }
}
