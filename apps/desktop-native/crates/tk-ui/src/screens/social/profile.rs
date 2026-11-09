//! AniList profile pages (worker **n**; checklist `social.profile.page`, `social.profile.public`, `social.follow`,
//! `social.follow.lists`; classic `ProfilePage.tsx`, `PublicProfilePage.tsx`, `FollowConnectionsPage.tsx`):
//! `Backend::anilist_profile(None)` for `/profile`, `anilist_profile(Some(name))` for `/user/:name`; avatar, banner,
//! about, stats, favourites, follow / unfollow (`anilist_follow`, `data-following`), followers / following sheets
//! (`anilist_followers` / `anilist_following`, paginated), the user's activity (`anilist_activity(User(id))`), the
//! viewer's AniList lists (`list_entries`, worker g) and "Edit on AniList" (`open_url`). `data-profile-user="<id>"`.
//! Everything shown is what AniList publishes (plan 14 section 0.1 #10): there is no own profile.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ActivityFeedVm, ListEntryVm, ListServiceVm, ListStatusVm, ProfileStatsVm, ProfileVm, UserVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, MediaCard, Pill, SegOption, Segmented, Sheet,
    SkeletonGrid, SkeletonLines, Tone, format_score, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::social::hours_watched;
use crate::model::{UiError, format_unix_utc};
use crate::screens::social::activity::ActivityList;
use crate::screens::social::{Avatar, Disconnected, SocialTabs, is_auth, is_not_found, media_route, paragraphs};

/// AniList's own page to edit the profile, avatar, about and privacy (no editor of our own).
pub const ANILIST_SETTINGS_URL: &str = "https://anilist.co/settings";

/// Tabs of the profile body (`social.profile.tab.<id>`, `data-profile-tab`).
pub const PROFILE_TABS: [&str; 3] = ["activity", "favorites", "lists"];

/// "Member since" date (`YYYY-MM-DD`, UTC).
pub fn member_since(created_unix: i64) -> String {
    let full = format_unix_utc(created_unix);
    full.get(..10).unwrap_or(&full).to_owned()
}

/// The stats grid: `(social.profile.stat.<id>, value)`; missing / zero manga stats are kept (AniList shows them too).
pub fn stat_cells(s: &ProfileStatsVm) -> Vec<(&'static str, String)> {
    vec![
        ("anime", s.anime_count.to_string()),
        ("episodes", s.episodes_watched.to_string()),
        ("hours", hours_watched(s.minutes_watched).to_string()),
        ("mean", s.mean_score.map(|m| format_score(m.round().clamp(0.0, 100.0) as u32)).unwrap_or_else(|| "-".to_owned())),
        ("manga", s.manga_count.to_string()),
        ("chapters", s.chapters_read.to_string()),
    ]
}

/// AniList's anime list page of a user (`https://anilist.co/user/<name>` + `/animelist`).
pub fn lists_url(user_url: &str) -> String {
    format!("{}/animelist", user_url.trim_end_matches('/'))
}

/// New follower count after toggling follow on a profile (never below zero).
pub fn followers_after(count: u32, follow: bool) -> u32 {
    if follow { count.saturating_add(1) } else { count.saturating_sub(1) }
}

/// The viewer's AniList entries with the status filter applied (`"all"` = every status).
pub fn filter_entries(entries: &[ListEntryVm], status: &str) -> Vec<ListEntryVm> {
    entries
        .iter()
        .filter(|e| e.service == ListServiceVm::AniList)
        .filter(|e| status == "all" || e.status.id() == status)
        .cloned()
        .collect()
}

/// `/profile`: the connected AniList account.
#[component]
pub fn ProfilePage() -> Element {
    rsx! {
        ProfileScreen { name: None }
    }
}

/// `/user/:name`: anyone's public AniList profile (works without an account).
#[component]
pub fn UserPage(name: String) -> Element {
    // Keyed: following a link to another user remounts with fresh state.
    rsx! {
        {std::iter::once(name.clone()).map(|n| rsx! {
            ProfileScreen { key: "{n}", name: Some(n.clone()) }
        })}
    }
}

#[component]
fn ProfileScreen(#[props(default)] name: Option<String>) -> Element {
    let services = use_context::<Services>();
    let mut profile = use_resource({
        let backend = services.backend.clone();
        let name = name.clone();
        move || {
            let (backend, name) = (backend.clone(), name.clone());
            async move { backend.anilist_profile(name.as_deref()).await }
        }
    });
    let own_route = name.is_none();
    let state = profile.read().clone();
    rsx! {
        div { class: "tk-page tk-social tk-social-profile", "data-page": "social.profile.title",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("social.profile.title")} }
                    SocialTabs { current: if own_route { "profile".to_string() } else { "user".to_string() } }
                }
            }
            match state {
                None => rsx! {
                    div { "aria-busy": "true", "data-loading": "true",
                        SkeletonLines { widths: vec![40, 90, 75, 60] }
                    }
                },
                Some(Err(e)) if is_auth(&e) => rsx! {
                    Disconnected {}
                },
                Some(Err(e)) if is_not_found(&e) => rsx! {
                    EmptyState {
                        icon: "user".to_string(),
                        title: t("social.profile.not_found.title"),
                        body: tf("social.profile.not_found.body", &[("name", name.as_deref().unwrap_or_default())]),
                    }
                },
                Some(Err(e)) => rsx! {
                    ErrorState { error: e, on_retry: move |_| profile.restart() }
                },
                Some(Ok(p)) => {
                    let uid = p.user.id;
                    rsx! {
                        ProfileBody { key: "{uid}", profile: p }
                    }
                }
            }
        }
    }
}

#[component]
fn ProfileBody(profile: ProfileVm) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let p = profile;
    let mut following = use_signal(|| p.is_following);
    let mut followers = use_signal(|| p.followers);
    let mut follow_busy = use_signal(|| false);
    let mut sheet = use_signal(|| None::<&'static str>);
    let mut tab = use_signal(|| "activity".to_string());
    let user_id = p.user.id;

    let open = {
        let backend = services.backend.clone();
        move |url: String| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening AniList failed");
                    snack.show(t("social.open_failed"));
                }
            });
        }
    };
    let toggle_follow = {
        let backend = services.backend.clone();
        move |_| {
            if follow_busy() {
                return;
            }
            let want = !following();
            let backend = backend.clone();
            let before = followers();
            // Optimistic; reverted on failure.
            following.set(want);
            followers.set(followers_after(before, want));
            follow_busy.set(true);
            spawn(async move {
                match backend.anilist_follow(user_id, want).await {
                    Ok(()) => {}
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "AniList follow failed");
                        following.set(!want);
                        followers.set(before);
                        snack.show(if is_auth(&e) { t("social.disconnected.body") } else { t("social.profile.follow_failed") });
                    }
                }
                follow_busy.set(false);
            });
        }
    };

    let tabs: Vec<SegOption> = PROFILE_TABS.iter().map(|id| (id.to_string(), t(&format!("social.profile.tab.{id}")))).collect();
    let current_tab = tab();
    let open_edit = open.clone();
    let open_page = open.clone();
    let user_url = p.user.url.clone();
    let following_now = following();

    rsx! {
        section {
            class: "tk-social-profile__card",
            "data-profile-user": "{user_id}",
            "data-profile-name": "{p.user.name}",
            "data-me": "{p.is_me}",
            if let Some(b) = p.banner.clone() {
                img { class: "tk-social-profile__banner", src: "{b}", alt: "", loading: "lazy" }
            } else {
                div { class: "tk-social-profile__banner tk-social-profile__banner--empty" }
            }
            div { class: "tk-social-profile__head",
                Avatar { user: p.user.clone(), size: 96 }
                div { class: "tk-social-profile__who",
                    h2 { class: "tk-social-profile__name", "{p.user.name}" }
                    div { class: "tk-social-profile__meta",
                        if p.is_follower && !p.is_me {
                            Pill { text: t("social.profile.follows_you"), tone: Tone::Primary }
                        }
                        if let Some(c) = p.created_unix {
                            span { class: "tk-muted", {tf("social.profile.member_since", &[("date", &member_since(c))])} }
                        }
                    }
                    div { class: "tk-social-profile__counts",
                        button {
                            class: "tk-social-profile__count",
                            r#type: "button",
                            "data-followers": "{followers()}",
                            onclick: move |_| sheet.set(Some("followers")),
                            b { "{followers()}" }
                            span { {t("social.profile.followers")} }
                        }
                        button {
                            class: "tk-social-profile__count",
                            r#type: "button",
                            "data-following-count": "{p.following}",
                            onclick: move |_| sheet.set(Some("following")),
                            b { "{p.following}" }
                            span { {t("social.profile.following")} }
                        }
                    }
                }
                div { class: "tk-social-profile__actions",
                    if p.is_me {
                        Button {
                            label: t("social.profile.edit"),
                            icon: "edit".to_string(),
                            variant: ButtonVariant::Tonal,
                            onclick: move |_| open_edit(ANILIST_SETTINGS_URL.to_string()),
                        }
                    } else {
                        span { "data-following": "{following_now}",
                            Button {
                                label: if following_now { t("social.profile.following_btn") } else { t("social.profile.follow") },
                                icon: if following_now { "check".to_string() } else { "plus".to_string() },
                                variant: if following_now { ButtonVariant::Tonal } else { ButtonVariant::Filled },
                                disabled: follow_busy(),
                                onclick: toggle_follow,
                            }
                        }
                    }
                    Button {
                        label: t("social.open_anilist"),
                        icon: "external".to_string(),
                        variant: ButtonVariant::Text,
                        onclick: move |_| open_page(user_url.clone()),
                    }
                }
            }
            if let Some(about) = p.about.clone().filter(|a| !a.trim().is_empty()) {
                div { class: "tk-social-profile__about", "data-about": "true",
                    for (i , para) in paragraphs(&about).into_iter().enumerate() {
                        p { key: "{i}", "{para}" }
                    }
                }
            }
            dl { class: "tk-social-profile__stats", "data-stats": "true",
                for (id , value) in stat_cells(&p.stats) {
                    div { key: "{id}", class: "tk-social-profile__stat", "data-stat": "{id}",
                        dt { class: "tk-muted", {t(&format!("social.profile.stat.{id}"))} }
                        dd { "{value}" }
                    }
                }
            }
        }
        div { class: "tk-social-profile__tabs",
            Segmented {
                options: tabs,
                selected: current_tab.clone(),
                label: t("social.profile.tabs"),
                on_select: move |id: String| tab.set(id),
            }
        }
        div { class: "tk-social-profile__tab", "data-profile-tab": "{current_tab}",
            match current_tab.as_str() {
                "favorites" => rsx! {
                    Favorites { items: p.favorites.clone() }
                },
                "lists" => rsx! {
                    Lists { is_me: p.is_me, user: p.user.clone() }
                },
                _ => rsx! {
                    ActivityList { key: "{user_id}", feed: ActivityFeedVm::User(user_id), composer: p.is_me, compact: true }
                },
            }
        }
        if let Some(edge) = sheet() {
            FollowSheet { user_id, edge: edge.to_string(), on_close: move |_| sheet.set(None) }
        }
    }
}

#[component]
fn Favorites(items: Vec<crate::backend::MediaVm>) -> Element {
    let navigator = use_navigator();
    if items.is_empty() {
        return rsx! {
            EmptyState { icon: "heart".to_string(), title: t("social.profile.favorites.empty.title"), body: t("social.profile.favorites.empty.body") }
        };
    }
    rsx! {
        div { class: "tk-grid", "data-favorites": "{items.len()}",
            for m in items.iter().cloned() {
                {
                    let target = media_route(&m.id);
                    rsx! {
                        MediaCard {
                            key: "{m.id}",
                            media: m.clone(),
                            onclick: move |_| {
                                navigator.push(target.clone());
                            },
                        }
                    }
                }
            }
        }
    }
}

/// The viewer's AniList lists (worker g's mirror); other users' lists live on anilist.co.
#[component]
fn Lists(is_me: bool, user: UserVm) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    if !is_me {
        let url = lists_url(&user.url);
        let backend = services.backend.clone();
        return rsx! {
            EmptyState {
                icon: "library".to_string(),
                title: tf("social.profile.lists.public.title", &[("name", &user.name)]),
                body: t("social.profile.lists.public.body"),
                action_label: t("social.open_anilist"),
                on_action: move |_| {
                    let (backend, url) = (backend.clone(), url.clone());
                    spawn(async move {
                        if backend.open_url(&url).await.is_err() {
                            snack.show(t("social.open_failed"));
                        }
                    });
                },
            }
        };
    }
    rsx! {
        OwnLists {}
    }
}

#[component]
fn OwnLists() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut status = use_signal(|| "all".to_string());
    let mut entries = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_entries(None).await }
        }
    });
    let mut options: Vec<SegOption> = vec![("all".to_string(), t("social.profile.lists.all"))];
    options.extend(ListStatusVm::ALL.iter().map(|s| (s.id().to_string(), t(&format!("lists.status.{}", s.id())))));
    let selected = status();
    let state = entries.read().clone();
    rsx! {
        div { class: "tk-social-lists", "data-lists-status": "{selected}",
            Segmented {
                options,
                selected: selected.clone(),
                label: t("social.profile.tab.lists"),
                small: true,
                on_select: move |id: String| status.set(id),
            }
            match state {
                None => rsx! { SkeletonGrid { count: 12 } },
                Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| entries.restart() } },
                Some(Ok(all)) => {
                    let shown = filter_entries(&all, &selected);
                    if shown.is_empty() {
                        rsx! {
                            EmptyState { icon: "library".to_string(), title: t("social.profile.lists.empty.title"), body: t("social.profile.lists.empty.body") }
                        }
                    } else {
                        rsx! {
                            div { class: "tk-grid", "data-list-entries": "{shown.len()}",
                                for e in shown.iter().cloned() {
                                    {
                                        let target = media_route(&e.media.id);
                                        let sub = match e.episodes_total {
                                            Some(total) => format!("{}/{} · {}", e.progress, total, t(&format!("lists.status.{}", e.status.id()))),
                                            None => format!("{} · {}", e.progress, t(&format!("lists.status.{}", e.status.id()))),
                                        };
                                        rsx! {
                                            MediaCard {
                                                key: "{e.media.id}",
                                                media: e.media.clone(),
                                                sub,
                                                onclick: move |_| {
                                                    navigator.push(target.clone());
                                                },
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Followers / following of a user, paginated (`Page.followers` / `Page.following`), in a side sheet.
#[component]
fn FollowSheet(user_id: u64, edge: String, on_close: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut users = use_signal(Vec::<UserVm>::new);
    let mut page = use_signal(|| 0_u32);
    let mut has_next = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let followers_edge = edge == "followers";

    let load = use_callback({
        let backend = services.backend.clone();
        move |next: u32| {
            if *busy.peek() {
                return;
            }
            let backend = backend.clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                let result = if followers_edge {
                    backend.anilist_followers(user_id, next).await
                } else {
                    backend.anilist_following(user_id, next).await
                };
                match result {
                    Ok(p) => {
                        let mut list = users.write();
                        for u in p.items {
                            if !list.iter().any(|x| x.id == u.id) {
                                list.push(u);
                            }
                        }
                        drop(list);
                        page.set(next);
                        has_next.set(p.has_next);
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });
    use_hook(move || load.call(1));

    let list = users.read().clone();
    let err = error.read().clone();
    let title = if followers_edge { t("social.profile.followers") } else { t("social.profile.following") };
    rsx! {
        Sheet { title, on_close: move |_| on_close.call(()),
            div { class: "tk-social-follows", "data-follow-list": "{edge}", "data-count": "{list.len()}",
                if list.is_empty() && busy() {
                    SkeletonLines { widths: vec![70, 60, 80, 50] }
                } else if let (true, Some(e)) = (list.is_empty(), err.clone()) {
                    if is_auth(&e) {
                        Disconnected { compact: true }
                    } else {
                        ErrorState { error: e, compact: true, on_retry: move |_| load.call(1) }
                    }
                } else if list.is_empty() {
                    EmptyState {
                        icon: "user".to_string(),
                        title: if followers_edge { t("social.profile.followers.empty") } else { t("social.profile.following.empty") },
                    }
                } else {
                    ul { class: "tk-social-follows__list",
                        for u in list.iter().cloned() {
                            {
                                let name = u.name.clone();
                                rsx! {
                                    li { key: "{u.id}", "data-user-id": "{u.id}",
                                        button {
                                            class: "tk-social-follows__row",
                                            r#type: "button",
                                            onclick: move |_| {
                                                on_close.call(());
                                                navigator.push(Route::UserPage { name: name.clone() });
                                            },
                                            Avatar { user: u.clone(), size: 36 }
                                            b { "{u.name}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(e) = err {
                        ErrorState { error: e, compact: true, on_retry: move |_| load.call(page() + 1) }
                    } else if has_next() {
                        div { class: "tk-social__more",
                            Button {
                                label: if busy() { t("social.loading") } else { t("social.load_more") },
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: busy(),
                                onclick: move |_| load.call(page() + 1),
                            }
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
    use crate::backend::MediaVm;

    fn entry(id: &str, service: ListServiceVm, status: ListStatusVm) -> ListEntryVm {
        ListEntryVm {
            media: MediaVm { id: id.into(), title: id.into(), cover: None, color: None, subtitle: String::new(), score: None },
            service,
            status,
            progress: 0,
            episodes_total: None,
            score: None,
            started: None,
            finished: None,
            repeats: 0,
            notes: None,
            updated_unix: 0,
            remote_id: None,
            pending: false,
        }
    }

    #[test]
    fn member_since_is_a_date() {
        assert_eq!(member_since(1_700_000_000), "2023-11-14");
        assert_eq!(member_since(0), "1970-01-01");
    }

    #[test]
    fn stats_grid() {
        let s = ProfileStatsVm { anime_count: 3, episodes_watched: 40, minutes_watched: 125, manga_count: 0, chapters_read: 0, mean_score: Some(78.6) };
        let cells = stat_cells(&s);
        assert_eq!(cells.len(), 6);
        assert_eq!(cells[2], ("hours", "2".to_owned()));
        assert_eq!(cells[3], ("mean", "7.9".to_owned()));
        assert_eq!(stat_cells(&ProfileStatsVm::default())[3].1, "-");
    }

    #[test]
    fn urls_and_counts() {
        assert_eq!(lists_url("https://anilist.co/user/Mia/"), "https://anilist.co/user/Mia/animelist");
        assert_eq!(followers_after(0, false), 0);
        assert_eq!(followers_after(4, true), 5);
        assert!(PROFILE_TABS.contains(&"lists"));
    }

    #[test]
    fn list_filter_keeps_anilist_entries() {
        let all = vec![
            entry("1", ListServiceVm::AniList, ListStatusVm::Watching),
            entry("2", ListServiceVm::Mal, ListStatusVm::Watching),
            entry("3", ListServiceVm::AniList, ListStatusVm::Completed),
        ];
        assert_eq!(filter_entries(&all, "all").len(), 2);
        let w = filter_entries(&all, "watching");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].media.id, "1");
    }
}
