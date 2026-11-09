package app.tatakai.next.nav

import android.content.Intent
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.NotificationExtras

/**
 * parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.12): notification taps and deep links of the wave, turned into a
 * [Route] by `MainActivity` and consumed by the shell (`routeLinks`). Extras = `NotificationExtras` (core/model); the only deep link is
 * `tatakai://user/<name>` (classic `/user/:name`, checklist social.profile.public). Torrent notifications keep [TorrentLink].
 */
object WaveBLinks {
    const val USER_HOST: String = "user"

    fun from(intent: Intent?): Route? {
        if (intent == null) return null
        intent.getStringExtra(NotificationExtras.DOWNLOADS_TAB)?.let { id ->
            return Route.Downloads(DownloadsTab.entries.firstOrNull { it.id == id } ?: DownloadsTab.ACTIVE)
        }
        intent.getStringExtra(NotificationExtras.CHAPTER_KIND)?.let { kind ->
            return Route.Downloads(if (kind == ChapterKind.NOVEL.name) DownloadsTab.NOVELS else DownloadsTab.MANGA)
        }
        val anilistId = intent.getIntExtra(NotificationExtras.DETAILS_ANILIST_ID, 0)
        if (anilistId > 0) return Route.Details(MediaKind.ANIME, anilistId)
        if (intent.getBooleanExtra(NotificationExtras.SOCIAL_NOTIFICATIONS, false)) return Route.Notifications
        if (intent.getBooleanExtra(NotificationExtras.LINK_HANDOFF, false)) return Route.Linked
        if (intent.action == Intent.ACTION_VIEW) {
            val data = intent.data ?: return null
            if (data.scheme.equals("tatakai", ignoreCase = true) && data.host.equals(USER_HOST, ignoreCase = true)) {
                val name = data.pathSegments.firstOrNull()?.takeIf { it.isNotBlank() } ?: return Route.Profile()
                return Route.Profile(name)
            }
        }
        return null
    }
}
