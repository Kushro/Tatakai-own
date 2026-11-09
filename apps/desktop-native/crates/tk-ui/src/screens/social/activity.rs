//! AniList activity feed `/activity` (worker **n**; checklist `social.activity.feed`, `social.activity.interact`;
//! classic `lib/anilist/client.ts` activity queries): tabs Following · Global (`Backend::anilist_activity(feed, page)`,
//! `data-feed`), like (`anilist_like_activity`, `data-liked`), replies sheet (`anilist_activity_replies`,
//! `anilist_reply`), "Post a status" (`anilist_post_status`), delete own (`anilist_delete_activity`). Contract
//! placeholder.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn ActivityPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "social.activity.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("social.activity.title")} }
            }
            EmptyState { icon: "history".to_string(), title: t("social.activity.title"), body: t("common.placeholder") }
        }
    }
}
