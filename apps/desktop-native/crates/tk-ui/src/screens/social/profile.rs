//! AniList profile pages (worker **n**; checklist `social.profile.page`, `social.profile.public`, `social.follow`,
//! `social.follow.lists`; classic `ProfilePage.tsx`, `PublicProfilePage.tsx`, `FollowConnectionsPage.tsx`):
//! `Backend::anilist_profile(None)` for `/profile`, `anilist_profile(Some(name))` for `/user/:name`; avatar, banner,
//! about, stats, favourites, follow / unfollow (`anilist_follow`, `data-following`), followers / following sheets
//! (`anilist_followers` / `anilist_following`, paginated), the user's activity (`anilist_activity(User(id))`), and
//! "Edit on AniList" (`open_url`). `data-profile-user="<id>"`. Contract placeholders: n replaces both bodies.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn ProfilePage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "social.profile.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("social.profile.title")} }
            }
            EmptyState { icon: "user".to_string(), title: t("social.profile.title"), body: t("common.placeholder") }
        }
    }
}

#[component]
pub fn UserPage(name: String) -> Element {
    let _ = name;
    rsx! {
        div { class: "tk-page", "data-page": "social.profile.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("social.profile.title")} }
            }
            EmptyState { icon: "user".to_string(), title: t("social.profile.title"), body: t("common.placeholder") }
        }
    }
}
