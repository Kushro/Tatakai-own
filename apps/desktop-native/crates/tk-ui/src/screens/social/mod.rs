//! AniList social (wave B, worker **n**; plan 14 section 0.1 #10: only what AniList offers; checklist `social.*`,
//! `details.reviews`): [`profile`] (`/profile` = the viewer, `/user/:name` = a public profile), [`notifications`]
//! (`/notifications`) and [`activity`] (`/activity`). All under `NavDest::Library`; the Library page links to them (n
//! asks the integrator for that one row) and every social page carries the same three-link header ([`SocialTabs`]).
//! n owns every file here plus `components/reviews.rs` (the Details mount).
//! Without an AniList account, every page that needs one shows the "connect AniList" [`Disconnected`] state with a
//! link to Settings > Accounts (`data-social="disconnected"`).

pub mod activity;
pub mod notifications;
pub mod profile;

use dioxus::prelude::*;

use crate::app::Route;
use crate::backend::UserVm;
use crate::components::{ButtonVariant, EmptyState, monogram};
use crate::i18n::{t, tf};
use crate::model::social::{SocialTarget, social_target};
use crate::model::{ErrorKind, UiError};

/// The error means "no AniList account (or the token was rejected)".
pub fn is_auth(e: &UiError) -> bool {
    e.kind == ErrorKind::Auth
}

/// AniList answered "no such user".
pub fn is_not_found(e: &UiError) -> bool {
    e.kind == ErrorKind::Other && e.detail.starts_with("not_found")
}

/// Page of a social `MediaVm::id` (anime -> Details, `manga:` -> manga details, `novel:` -> novel details).
pub fn media_route(id: &str) -> Route {
    match social_target(id) {
        SocialTarget::Anime(id) => Route::DetailsPage { id: id.to_owned() },
        SocialTarget::Manga(id) => Route::MangaDetailsPage { id: id.to_owned() },
        SocialTarget::Novel(id) => Route::NovelDetailsPage { id: id.to_owned() },
    }
}

/// Relative time as `(i18n key, n)`, same buckets as the Library (`time.*` in `common.json`).
pub fn rel_time_parts(elapsed_secs: i64) -> (&'static str, Option<i64>) {
    let m = elapsed_secs.max(0) / 60;
    if m < 1 {
        ("time.now", None)
    } else if m < 60 {
        ("time.min_ago", Some(m))
    } else if m < 60 * 24 {
        ("time.h_ago", Some((m + 30) / 60))
    } else if m < 60 * 48 {
        ("time.yesterday", None)
    } else {
        ("time.d_ago", Some((m + 720) / 1440))
    }
}

pub fn rel_time(now_unix: i64, then_unix: i64) -> String {
    match rel_time_parts(now_unix - then_unix) {
        (key, Some(n)) => tf(key, &[("n", &n.to_string())]),
        (key, None) => t(key),
    }
}

/// Paragraphs of a cleaned AniList text (blank lines split them, single newlines stay inside).
pub fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n").map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect()
}

/// Refresh the rail badge when the window regains focus (`social.notifications.badge`: "polled and when coming back
/// to the app"): wakes `model::social::wait_unread_poll`. Meant for the component that runs the badge poll
/// (`components::rail_badges::use_rail_badges`, integrator).
pub fn use_unread_focus_wake() {
    use dioxus::desktop::tao::event::Event;
    use dioxus::desktop::WindowEvent;
    let _handler = dioxus::desktop::use_wry_event_handler(|event, _| {
        if let Event::WindowEvent { event: WindowEvent::Focused(true), .. } = event {
            crate::model::social::wake_unread_poll();
        }
    });
}

/// Round avatar (image or monogram).
#[component]
pub fn Avatar(user: UserVm, #[props(default = 40)] size: u32) -> Element {
    rsx! {
        span { class: "tk-social__avatar", style: "width:{size}px;height:{size}px", "aria-hidden": "true",
            if let Some(src) = user.avatar.clone() {
                img { src: "{src}", alt: "", loading: "lazy" }
            } else {
                span { {monogram(&user.name)} }
            }
        }
    }
}

/// "Connect AniList" state (`data-social="disconnected"`), with a button to Settings (Accounts section).
#[component]
pub fn Disconnected(#[props(default)] compact: bool) -> Element {
    let navigator = use_navigator();
    rsx! {
        div { class: "tk-social__disconnected", "data-social": "disconnected", "data-compact": "{compact}",
            if compact {
                p { class: "tk-muted", {t("social.disconnected.body")} }
                crate::components::Button {
                    label: t("social.disconnected.action"),
                    variant: ButtonVariant::Tonal,
                    icon: "user".to_string(),
                    onclick: move |_| {
                        navigator.push(Route::SettingsPage {});
                    },
                }
            } else {
                EmptyState {
                    icon: "user".to_string(),
                    title: t("social.disconnected.title"),
                    body: t("social.disconnected.body"),
                    action_label: t("social.disconnected.action"),
                    on_action: move |_| {
                        navigator.push(Route::SettingsPage {});
                    },
                }
            }
        }
    }
}

/// The three social pages, as a link row in each page head (`data-social-nav`).
#[component]
pub fn SocialTabs(current: String) -> Element {
    let navigator = use_navigator();
    let tabs: [(&str, &str, Route); 3] = [
        ("profile", "social.nav.profile", Route::ProfilePage {}),
        ("activity", "social.nav.activity", Route::ActivityPage {}),
        ("notifications", "social.nav.notifications", Route::NotificationsPage {}),
    ];
    rsx! {
        nav { class: "tk-social__nav", "data-social-nav": "{current}", "aria-label": "{t(\"social.nav.label\")}",
            for (id , key , route) in tabs {
                {
                    let on = id == current;
                    rsx! {
                        crate::components::Chip {
                            key: "{id}",
                            label: t(key),
                            on,
                            onclick: move |_| {
                                navigator.push(route.clone());
                            },
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_and_not_found_errors() {
        assert!(is_auth(&UiError::new(ErrorKind::Auth, "not_connected")));
        assert!(!is_auth(&UiError::new(ErrorKind::Offline, "x")));
        assert!(is_not_found(&UiError::new(ErrorKind::Other, "not_found: Ana")));
        assert!(!is_not_found(&UiError::new(ErrorKind::Other, "boom")));
    }

    #[test]
    fn media_routes_by_prefix() {
        assert_eq!(media_route("21"), Route::DetailsPage { id: "21".into() });
        assert_eq!(media_route("manga:5"), Route::MangaDetailsPage { id: "5".into() });
        assert_eq!(media_route("novel:9"), Route::NovelDetailsPage { id: "9".into() });
    }

    #[test]
    fn relative_time_and_paragraphs() {
        assert_eq!(rel_time_parts(-5), ("time.now", None));
        assert_eq!(rel_time_parts(120), ("time.min_ago", Some(2)));
        assert_eq!(rel_time_parts(3 * 86_400), ("time.d_ago", Some(3)));
        assert_eq!(paragraphs("a\nb\n\n\nc"), vec!["a\nb".to_owned(), "c".to_owned()]);
    }
}
