// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

/**
 * Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.12): extras of the launcher `Intent` that a local
 * notification of a core module puts on its tap `PendingIntent` so the shell opens the right screen. `MainActivity` reads them
 * (`WaveBLinks.from(intent)`) and the shell navigates; the modules never depend on `:app`. Exactly one of the groups is present.
 * (The torrent notifications keep their own extras `app.tatakai.next.torrent.*`, wave android.)
 */
object NotificationExtras {
    /** `DownloadsTab.id` (worker r, :core:download: "episodio descargado" / "descarga fallida"). */
    const val DOWNLOADS_TAB: String = "app.tatakai.next.nav.DOWNLOADS_TAB"

    /** AniList id (Int) of a series whose episode aired (worker r, :core:discover `AiringReminderWorker`) -> `Route.Details(ANIME, id)`. */
    const val DETAILS_ANILIST_ID: String = "app.tatakai.next.nav.DETAILS_ANILIST_ID"

    /** Boolean true (worker t, :core:social `SocialNotificationsWorker`: "N notificaciones nuevas en AniList") -> `Route.Notifications`. */
    const val SOCIAL_NOTIFICATIONS: String = "app.tatakai.next.nav.SOCIAL_NOTIFICATIONS"

    /** `ChapterKind.name` + the chapter download id (worker s: "capítulo descargado") -> `Route.Downloads(MANGA | NOVELS)`. */
    const val CHAPTER_KIND: String = "app.tatakai.next.nav.CHAPTER_KIND"

    /** Boolean true (worker r, :core:link: "La PC te envió <título>", `LinkRepository.pendingHandoff`) -> `Route.Linked`. */
    const val LINK_HANDOFF: String = "app.tatakai.next.nav.LINK_HANDOFF"
}
