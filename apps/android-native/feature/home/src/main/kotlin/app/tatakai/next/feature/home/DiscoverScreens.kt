// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.Media
import app.tatakai.next.ui.state.WaveBPlaceholder

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r). Signatures FINAL (the shell and Details
// wire them); bodies: r.

/**
 * `Route.Calendar` (checklist home.calendar.schedule / .my-shows; plan §7 D6: the calendar lives inside Inicio): week view grouped by
 * day (`AiringRepository.schedule`), local time, followed shows highlighted with "Dejar de seguir" / reminder toggle, "Solo las que sigo"
 * filter. Reached from the Home row header "Calendario" and from Library (You list).
 */
@Composable
fun CalendarScreen(onBack: () -> Unit, onMediaClick: (Media) -> Unit, modifier: Modifier = Modifier) {
    WaveBPlaceholder(title = stringResource(R.string.home_calendar_title), modifier = modifier, onBack = onBack)
}

/**
 * `Route.ForYou` (checklist home.recs.for-you / .explanations / .feedback / .atlas): the full "Para ti" list (`RecsRepository.forYou`)
 * with affinity %, reasons chips, "No me interesa" (undo snackbar), the taste profile summary and the atlas tab (`atlas()`, Canvas with
 * clusters; tap a dot → [onMediaClick]).
 */
@Composable
fun ForYouScreen(onBack: () -> Unit, onMediaClick: (Media) -> Unit, modifier: Modifier = Modifier) {
    WaveBPlaceholder(title = stringResource(R.string.home_for_you_title), modifier = modifier, onBack = onBack)
}

/**
 * Checklist details.track-show: "Seguir" / "Siguiendo" (bell) row mounted by `WaveBSlots` of Details (o's file, closed) under the
 * episode list; toggles `AiringRepository.follow / unfollow` and the reminder switch (`setReminder`). Only for `kind == ANIME` and
 * `status == RELEASING | NOT_YET_RELEASED`. Empty until r implements it (nothing is drawn).
 */
@Composable
fun FollowShowRow(media: Media, modifier: Modifier = Modifier) {
    // wave B (r): implement as described above.
}
