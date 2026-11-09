// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.ActivityReply
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Review
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialRepository
import app.tatakai.next.model.SocialUser
import app.tatakai.next.social.SocialActivityLookup
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// Parity wave B (16 §3.7, worker t; checklist social.*, details.reviews): view models of the AniList social screens.

/** A paginated list: [items] so far, [error] of the last page (the first page shows it full screen, later ones inline). */
data class Paged<T>(
    val items: List<T> = emptyList(),
    val page: Int = 0,
    val hasNext: Boolean = true,
    val loading: Boolean = false,
    val error: AppError? = null,
) {
    val initialLoading: Boolean get() = loading && items.isEmpty()
    val initialError: AppError? get() = if (items.isEmpty()) error else null

    /** Appends [next] (deduplicated by [key]). */
    fun <K> plus(next: SocialPage<T>, key: (T) -> K): Paged<T> {
        val merged = if (next.page <= 1) next.items else (items + next.items).distinctBy(key)
        return Paged(merged, next.page, next.hasNext, loading = false, error = null)
    }
}

/** True when [e] is the "no AniList account" failure: screens show "Conecta AniList". */
fun AppError.isNotLinked(): Boolean = this is AppError.Unknown && msg == SOCIAL_NOT_LINKED

internal fun Throwable.asError(): AppError = when (this) {
    is AppException -> error
    is java.io.IOException -> AppError.Offline
    else -> AppError.Unknown(message?.takeIf { it.isNotBlank() } ?: this::class.java.simpleName)
}

/** Loads page after page of one list; ignores a request while one is running. */
internal class Pager<T, K>(
    private val scope: kotlinx.coroutines.CoroutineScope,
    private val key: (T) -> K,
    private val fetch: suspend (page: Int) -> SocialPage<T>,
) {
    private val _state = MutableStateFlow(Paged<T>())
    val state: StateFlow<Paged<T>> = _state.asStateFlow()
    private var job: Job? = null

    fun refresh() {
        job?.cancel()
        _state.update { it.copy(loading = true, error = null) }
        job = scope.launch { load(1) }
    }

    fun loadMore() {
        val s = _state.value
        if (s.loading || !s.hasNext || s.error != null) return
        _state.update { it.copy(loading = true) }
        job = scope.launch { load(s.page + 1) }
    }

    fun retry() {
        val s = _state.value
        _state.update { it.copy(loading = true, error = null) }
        job = scope.launch { load(if (s.items.isEmpty()) 1 else s.page + 1) }
    }

    /** Local change of one row (liked flag, reply count...). */
    fun updateItems(transform: (List<T>) -> List<T>) = _state.update { it.copy(items = transform(it.items)) }

    private suspend fun load(page: Int) {
        try {
            val next = fetch(page)
            _state.update { it.plus(next, key) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(loading = false, error = e.asError()) }
        }
    }
}

// ---- profile -------------------------------------------------------------------------------------------------------------------------

enum class ProfileTab { ACTIVITY, STATS, FAVORITES, FOLLOWERS, FOLLOWING }

data class ProfileUiState(
    val user: LoadState<SocialUser> = LoadState.Loading,
    val tab: ProfileTab = ProfileTab.ACTIVITY,
    val followingOnly: Boolean = false,
    val followBusy: Boolean = false,
    val posting: Boolean = false,
)

sealed interface SocialNotice {
    data class Failed(val error: AppError) : SocialNotice
    data object Posted : SocialNotice
    data object Replied : SocialNotice
}

class ProfileViewModel(private val userName: String?, private val repo: SocialRepository) : ViewModel() {
    private val _state = MutableStateFlow(ProfileUiState())
    val state: StateFlow<ProfileUiState> = _state.asStateFlow()
    private val _notices = Channel<SocialNotice>(Channel.BUFFERED)
    val notices: Flow<SocialNotice> = _notices.receiveAsFlow()

    val activity = Pager<SocialActivity, Int>(viewModelScope, { it.id }) { page ->
        val u = (state.value.user as? LoadState.Ok)?.value
        val following = state.value.followingOnly
        repo.activity(userId = if (following) null else u?.id, followingOnly = following, page = page)
    }
    val followers = Pager<SocialUser, Int>(viewModelScope, { it.id }) { page -> repo.followers(currentUserId(), page) }
    val following = Pager<SocialUser, Int>(viewModelScope, { it.id }) { page -> repo.following(currentUserId(), page) }

    init {
        load()
    }

    fun load() {
        _state.update { it.copy(user = LoadState.Loading) }
        viewModelScope.launch {
            val next: LoadState<SocialUser> = try {
                repo.profile(userName)?.let { LoadState.Ok(it) } ?: LoadState.Err(AppError.Http(404))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.asError())
            }
            _state.update { it.copy(user = next) }
            if (next is LoadState.Ok) activity.refresh()
        }
    }

    fun selectTab(tab: ProfileTab) {
        _state.update { it.copy(tab = tab) }
        when (tab) {
            ProfileTab.FOLLOWERS -> if (followers.state.value.page == 0) followers.refresh()
            ProfileTab.FOLLOWING -> if (following.state.value.page == 0) following.refresh()
            ProfileTab.ACTIVITY -> if (activity.state.value.page == 0 && !activity.state.value.loading) activity.refresh()
            else -> Unit
        }
    }

    /** "Solo los que sigo" (only on the viewer's own profile). */
    fun setFollowingOnly(on: Boolean) {
        if (_state.value.followingOnly == on) return
        _state.update { it.copy(followingOnly = on) }
        activity.refresh()
    }

    fun toggleFollow() {
        val u = (state.value.user as? LoadState.Ok)?.value ?: return
        if (u.isViewer || _state.value.followBusy) return
        _state.update { it.copy(followBusy = true) }
        viewModelScope.launch {
            repo.toggleFollow(u.id).fold(
                onSuccess = { now ->
                    val delta = if (now == u.isFollowing) 0 else if (now) 1 else -1
                    _state.update { it.copy(user = LoadState.Ok(u.copy(isFollowing = now, followers = (u.followers + delta).coerceAtLeast(0))), followBusy = false) }
                },
                onFailure = { e ->
                    _state.update { it.copy(followBusy = false) }
                    _notices.trySend(SocialNotice.Failed(e.asError()))
                },
            )
        }
    }

    fun postStatus(text: String) {
        if (text.isBlank() || _state.value.posting) return
        _state.update { it.copy(posting = true) }
        viewModelScope.launch {
            repo.postStatus(text).fold(
                onSuccess = { posted ->
                    activity.updateItems { listOf(posted) + it.filterNot { a -> a.id == posted.id } }
                    _notices.trySend(SocialNotice.Posted)
                },
                onFailure = { e -> _notices.trySend(SocialNotice.Failed(e.asError())) },
            )
            _state.update { it.copy(posting = false) }
        }
    }

    fun toggleLike(activityId: Int) {
        activity.updateItems { list -> list.map { if (it.id == activityId) it.withLike(!it.liked) else it } }
        viewModelScope.launch {
            repo.toggleLike(activityId).fold(
                onSuccess = { liked -> activity.updateItems { list -> list.map { if (it.id == activityId) it.withLike(liked) else it } } },
                onFailure = { e ->
                    activity.updateItems { list -> list.map { if (it.id == activityId) it.withLike(!it.liked) else it } }
                    _notices.trySend(SocialNotice.Failed(e.asError()))
                },
            )
        }
    }

    private suspend fun currentUserId(): Int =
        (state.value.user as? LoadState.Ok)?.value?.id ?: repo.profile(userName)?.id ?: throw AppException(AppError.Http(404))
}

/** The same activity with [liked] set and the like count adjusted. */
fun SocialActivity.withLike(liked: Boolean): SocialActivity {
    if (liked == this.liked) return this
    val likes = (likes + if (liked) 1 else -1).coerceAtLeast(0)
    return when (this) {
        is SocialActivity.ListActivity -> copy(liked = liked, likes = likes)
        is SocialActivity.TextActivity -> copy(liked = liked, likes = likes)
    }
}

fun SocialActivity.withReplies(count: Int): SocialActivity = when (this) {
    is SocialActivity.ListActivity -> copy(replyCount = count)
    is SocialActivity.TextActivity -> copy(replyCount = count)
}

// ---- activity thread ----------------------------------------------------------------------------------------------------------------

data class ThreadUiState(val activity: LoadState<SocialActivity> = LoadState.Loading, val sending: Boolean = false)

class ActivityThreadViewModel(
    private val activityId: Int,
    private val repo: SocialRepository,
    private val lookup: SocialActivityLookup? = null,
) : ViewModel() {
    private val _state = MutableStateFlow(ThreadUiState())
    val state: StateFlow<ThreadUiState> = _state.asStateFlow()
    private val _notices = Channel<SocialNotice>(Channel.BUFFERED)
    val notices: Flow<SocialNotice> = _notices.receiveAsFlow()
    val replies = Pager<ActivityReply, Int>(viewModelScope, { it.id }) { page -> repo.replies(activityId, page) }

    init {
        load()
    }

    fun load() {
        _state.update { it.copy(activity = LoadState.Loading) }
        viewModelScope.launch {
            val next: LoadState<SocialActivity> = try {
                val found = lookup?.activityById(activityId)
                if (found != null) LoadState.Ok(found) else LoadState.Err(AppError.Http(404))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.asError())
            }
            _state.update { it.copy(activity = next) }
        }
        replies.refresh()
    }

    fun toggleLike() {
        val a = (state.value.activity as? LoadState.Ok)?.value ?: return
        _state.update { it.copy(activity = LoadState.Ok(a.withLike(!a.liked))) }
        viewModelScope.launch {
            repo.toggleLike(activityId).fold(
                onSuccess = { liked -> _state.update { it.copy(activity = LoadState.Ok(a.withLike(liked))) } },
                onFailure = { e ->
                    _state.update { it.copy(activity = LoadState.Ok(a)) }
                    _notices.trySend(SocialNotice.Failed(e.asError()))
                },
            )
        }
    }

    /** Returns immediately; the field is cleared by the screen on [SocialNotice.Replied]. */
    fun reply(text: String) {
        if (text.isBlank() || _state.value.sending) return
        _state.update { it.copy(sending = true) }
        viewModelScope.launch {
            repo.reply(activityId, text).fold(
                onSuccess = { r ->
                    replies.updateItems { it + r }
                    _state.update { s ->
                        val a = (s.activity as? LoadState.Ok)?.value
                        s.copy(activity = if (a != null) LoadState.Ok(a.withReplies(a.replyCount + 1)) else s.activity)
                    }
                    _notices.trySend(SocialNotice.Replied)
                },
                onFailure = { e -> _notices.trySend(SocialNotice.Failed(e.asError())) },
            )
            _state.update { it.copy(sending = false) }
        }
    }
}

// ---- notifications --------------------------------------------------------------------------------------------------------------------

/** Where tapping a notification goes. */
sealed interface NotificationTarget {
    data class OpenMedia(val media: Media) : NotificationTarget
    data class OpenActivity(val activityId: Int) : NotificationTarget
    data class OpenUser(val userName: String) : NotificationTarget
    data object None : NotificationTarget
}

/** AIRING / media notifications -> the title; likes / replies / mentions / messages -> the activity; follows / forum -> the user. */
fun notificationTarget(n: SocialNotification): NotificationTarget {
    val activity = n.activityId
    val media = n.media
    val user = n.user
    return when {
        n.type == "AIRING" && media != null -> NotificationTarget.OpenMedia(media)
        n.type.startsWith("ACTIVITY_") && activity != null -> NotificationTarget.OpenActivity(activity)
        media != null -> NotificationTarget.OpenMedia(media)
        user != null -> NotificationTarget.OpenUser(user.name)
        else -> NotificationTarget.None
    }
}

class NotificationsViewModel(private val repo: SocialRepository) : ViewModel() {
    val unread: StateFlow<Int> get() = repo.unreadCount

    /** First page marks everything read on AniList (`resetNotificationCount`), as opening the AniList page does. */
    val pager = Pager<SocialNotification, Int>(viewModelScope, { it.id }) { page -> repo.notifications(page, markRead = page == 1) }

    init {
        pager.refresh()
    }

    fun refresh() = pager.refresh()

    fun onResume() {
        viewModelScope.launch { runCatching { repo.refreshUnread() } }
    }
}

// ---- details slot --------------------------------------------------------------------------------------------------------------------

data class SlotUiState(val ownActivity: List<SocialActivity> = emptyList(), val linked: Boolean = false)

class SocialSlotViewModel(private val anilistId: Int, private val repo: SocialRepository) : ViewModel() {
    private val _state = MutableStateFlow(SlotUiState())
    val state: StateFlow<SlotUiState> = _state.asStateFlow()
    val reviews = Pager<Review, Int>(viewModelScope, { it.id }) { page -> repo.reviews(MediaRef(anilistId = anilistId), page) }

    init {
        reviews.refresh()
        viewModelScope.launch {
            val own = try {
                repo.mediaActivity(MediaRef(anilistId = anilistId))
            } catch (e: CancellationException) {
                throw e
            } catch (_: Exception) {
                emptyList()
            }
            _state.update { it.copy(ownActivity = own, linked = repo.viewer.value != null) }
        }
    }
}
