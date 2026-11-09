//! AniList notifications `/notifications` (worker **n**; checklist `social.notifications.list`,
//! `social.notifications.badge`; classic `NotificationsPage.tsx`, `NotificationSheet.tsx`):
//! `Backend::anilist_notifications(page, unread_only)` grouped by day, "Mark all read"
//! (`anilist_mark_notifications_read`), each row opens its title / profile / activity (`data-notification-kind`,
//! `data-unread`). The rail badge comes from `components::rail_badges` (`anilist_unread_count`). Contract placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn NotificationsPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "social.notifications.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("social.notifications.title")} }
            }
            EmptyState { icon: "info".to_string(), title: t("social.notifications.title"), body: t("common.placeholder") }
        }
    }
}
