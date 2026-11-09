// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.Media
import app.tatakai.next.ui.state.WaveBPlaceholder

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.7, worker t). Signatures FINAL; bodies: t. Every screen
// shows "Conecta AniList" (EmptyState → [onOpenAccounts]) when `SocialRepository.viewer` is null (`SOCIAL_NOT_LINKED`).

/**
 * `Route.Profile(userName)` (checklist social.profile.page / .public / social.follow / social.follow.lists / social.activity.feed /
 * social.activity.interact): null = the viewer. Avatar, banner, nombre, about, seguidores / seguidos (listas paginadas), estadísticas
 * (`SocialStats`), favoritos, pestaña Actividad (propia + de los que sigues con `followingOnly`), "Seguir" / "Siguiendo" (`toggleFollow`),
 * publicar estado (`postStatus`), "Editar perfil" → `https://anilist.co/settings` por `ACTION_VIEW`. [onBack] null on wide windows.
 */
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
    WaveBPlaceholder(title = stringResource(R.string.social_profile_title), modifier = modifier, onBack = onBack)
}

/** `Route.ActivityThread(activityId)` (checklist social.activity.interact): the activity, likes (`toggleLike`), replies, "Responder" (`reply`). */
@Composable
fun ActivityThreadScreen(
    activityId: Int,
    onBack: () -> Unit,
    onOpenUser: (userName: String) -> Unit,
    onOpenMedia: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.social_activity_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.Notifications` (checklist social.notifications.list / .badge): `SocialRepository.notifications(markRead = true)` paginated;
 * AIRING → [onOpenMedia], FOLLOWING → [onOpenUser], likes / replies / mentions → [onOpenActivity]; pull to refresh; `refreshUnread` on resume.
 */
@Composable
fun NotificationsScreen(
    onBack: () -> Unit,
    onOpenMedia: (Media) -> Unit,
    onOpenActivity: (activityId: Int) -> Unit,
    onOpenUser: (userName: String) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    WaveBPlaceholder(title = stringResource(R.string.social_notifications_title), modifier = modifier, onBack = onBack)
}

/**
 * Checklist details.reviews / details.comments-mount (id estable: "montaje de actividad / reseñas de AniList"): mounted by `WaveBSlots` of
 * Details (o's file, closed) at the end of the Information tab. Shows the AniList mean score, the reviews (`SocialRepository.reviews`:
 * autor, puntuación, resumen, expandir) with "Escribir reseña" → `https://anilist.co/review/editor/<id>` (ACTION_VIEW) and the viewer's
 * own activity on the title (`mediaActivity`). No own comments (plan 14 §0.1 #10). Empty until t implements it (nothing is drawn).
 */
@Composable
fun AniListSocialSlot(media: Media, modifier: Modifier = Modifier) {
    // wave B (t): implement as described above.
}
