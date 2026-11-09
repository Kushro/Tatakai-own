//! AniList notifications `/notifications` (worker **n**; checklist `social.notifications.list`,
//! `social.notifications.badge`; classic `NotificationsPage.tsx`, `NotificationSheet.tsx`):
//! `Backend::anilist_notifications(page, unread_only)` grouped by day, filter chips by type (airing, follows,
//! activity, forum, media), "Unread only", "Mark all read" (`anilist_mark_notifications_read`, then the rail badge
//! poll is woken so it drops to 0 at once), and each row opens its title / profile / activity on AniList
//! (`data-notification-kind`, `data-unread`). The rail badge itself comes from `components::rail_badges`
//! (`anilist_unread_count`); AniList has no push, so it is polled.

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::{NotificationPageVm, NotificationVm};
use crate::components::{Button, ButtonSize, ButtonVariant, Chip, EmptyState, ErrorState, MediaCover, SkeletonLines, Switch, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::UiError;
use crate::model::social::{NOTIFICATION_GROUPS, group_by_day, is_unread, notification_group, wake_unread_poll};
use crate::platform::now_unix;
use crate::screens::social::{Avatar, Disconnected, SocialTabs, is_auth, media_route, rel_time};

/// Where a notification row leads.
#[derive(Debug, Clone, PartialEq)]
pub enum NotificationTarget {
    /// A title page (`MediaVm::id` of the social view-models).
    Media(String),
    /// A public profile (`/user/<name>`).
    User(String),
    /// Anything else (an activity, a forum thread): AniList in the browser.
    Web(String),
    None,
}

/// Airing / media notifications open the title; follows open the profile; the rest open AniList.
pub fn target_of(n: &NotificationVm) -> NotificationTarget {
    let group = notification_group(&n.kind);
    match (group, &n.media, &n.user, &n.url) {
        ("airing" | "media", Some(m), _, _) => NotificationTarget::Media(m.id.clone()),
        ("follows", _, Some(u), _) => NotificationTarget::User(u.name.clone()),
        (_, _, _, Some(url)) => NotificationTarget::Web(url.clone()),
        (_, Some(m), _, None) => NotificationTarget::Media(m.id.clone()),
        (_, None, Some(u), None) => NotificationTarget::User(u.name.clone()),
        _ => NotificationTarget::None,
    }
}

/// `(item, unread)` pairs of the loaded rows, with the group filter applied (`"all"` keeps everything).
pub fn visible(items: &[NotificationVm], unread: u32, group: &str) -> Vec<(NotificationVm, bool)> {
    items
        .iter()
        .enumerate()
        .filter(|(_, n)| group == "all" || notification_group(&n.kind) == group)
        .map(|(i, n)| (n.clone(), is_unread(i, unread)))
        .collect()
}

/// `items` + the next page without duplicates.
pub fn merge(items: &mut Vec<NotificationVm>, page: Vec<NotificationVm>) {
    for n in page {
        if !items.iter().any(|x| x.id == n.id) {
            items.push(n);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Loading,
    Ready,
    Failed(UiError),
}

#[component]
pub fn NotificationsPage() -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut items = use_signal(Vec::<NotificationVm>::new);
    let mut unread = use_signal(|| 0_u32);
    let mut page = use_signal(|| 0_u32);
    let mut has_next = use_signal(|| false);
    let mut phase = use_signal(|| Phase::Loading);
    let mut more_busy = use_signal(|| false);
    let mut more_error = use_signal(|| None::<UiError>);
    let mut group = use_signal(|| "all".to_string());
    let mut unread_only = use_signal(|| false);
    let mut marking = use_signal(|| false);

    // `next == 1` reloads from the top.
    let load = use_callback({
        let backend = services.backend.clone();
        move |next: u32| {
            if next > 1 && *more_busy.peek() {
                return;
            }
            let backend = backend.clone();
            let only = *unread_only.peek();
            if next <= 1 {
                phase.set(Phase::Loading);
            } else {
                more_busy.set(true);
            }
            more_error.set(None);
            spawn(async move {
                let result: Result<NotificationPageVm, UiError> = backend.anilist_notifications(next, only).await;
                match result {
                    Ok(p) => {
                        if next <= 1 {
                            items.set(p.items);
                        } else {
                            merge(&mut items.write(), p.items);
                        }
                        unread.set(p.unread);
                        page.set(p.page.max(next));
                        has_next.set(p.has_next);
                        phase.set(Phase::Ready);
                    }
                    Err(e) if next <= 1 => phase.set(Phase::Failed(e)),
                    Err(e) => more_error.set(Some(e)),
                }
                more_busy.set(false);
            });
        }
    });
    use_hook(move || load.call(1));

    let mark_read = {
        let backend = services.backend.clone();
        move |_| {
            if marking() {
                return;
            }
            let backend = backend.clone();
            marking.set(true);
            spawn(async move {
                match backend.anilist_mark_notifications_read().await {
                    Ok(()) => {
                        unread.set(0);
                        wake_unread_poll();
                        snack.show(t("social.notifications.marked"));
                        if *unread_only.peek() {
                            items.set(Vec::new());
                            has_next.set(false);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "marking AniList notifications read failed");
                        snack.show(if is_auth(&e) { t("social.disconnected.body") } else { t("social.notifications.mark_failed") });
                    }
                }
                marking.set(false);
            });
        }
    };

    let now = now_unix();
    let current = phase.read().clone();
    let all = items.read().clone();
    let unread_now = unread();
    let selected = group();
    let rows = visible(&all, unread_now, &selected);
    let groups = group_by_day(now, &rows, |(n, _)| n.created_unix);
    let only = unread_only();

    rsx! {
        div {
            class: "tk-page tk-social tk-social-notifs",
            "data-page": "social.notifications.title",
            "data-unread-count": "{unread_now}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("social.notifications.title")} }
                    SocialTabs { current: "notifications".to_string() }
                }
                if !matches!(current, Phase::Failed(ref e) if is_auth(e)) {
                    div { class: "tk-social-notifs__actions",
                        Switch {
                            checked: only,
                            label: t("social.notifications.unread_only"),
                            on_toggle: move |on: bool| {
                                unread_only.set(on);
                                load.call(1);
                            },
                        }
                        Button {
                            label: if unread_now > 0 { tf("social.notifications.mark_read_n", &[("n", &unread_now.to_string())]) } else { t("social.notifications.mark_read") },
                            icon: "check".to_string(),
                            variant: ButtonVariant::Tonal,
                            disabled: marking() || unread_now == 0,
                            onclick: mark_read,
                        }
                    }
                }
            }
            if matches!(current, Phase::Ready) && !all.is_empty() {
                div { class: "tk-social-notifs__groups", role: "group", "aria-label": "{t(\"social.notifications.filter\")}",
                    for g in NOTIFICATION_GROUPS {
                        Chip {
                            key: "{g}",
                            label: t(&format!("social.notifications.group.{g}")),
                            on: selected == g,
                            onclick: move |_| group.set(g.to_string()),
                        }
                    }
                }
            }
            match current {
                Phase::Loading => rsx! {
                    div { "aria-busy": "true", "data-loading": "true",
                        SkeletonLines { widths: vec![85, 70, 90, 60, 75] }
                    }
                },
                Phase::Failed(e) if is_auth(&e) => rsx! {
                    Disconnected {}
                },
                Phase::Failed(e) => rsx! {
                    ErrorState { error: e, on_retry: move |_| load.call(1) }
                },
                Phase::Ready if rows.is_empty() => rsx! {
                    EmptyState {
                        icon: "info".to_string(),
                        title: if only { t("social.notifications.empty.unread") } else { t("social.notifications.empty.title") },
                        body: t("social.notifications.empty.body"),
                    }
                    if has_next() {
                        div { class: "tk-social__more",
                            Button {
                                label: t("social.load_more"),
                                variant: ButtonVariant::Tonal,
                                disabled: more_busy(),
                                onclick: move |_| load.call(page() + 1),
                            }
                        }
                    }
                },
                Phase::Ready => rsx! {
                    div { class: "tk-social-notifs__list", "data-group": "{selected}", "data-count": "{rows.len()}",
                        for (bucket , list) in groups {
                            section { key: "{bucket.id()}", class: "tk-social-notifs__day", "data-day": "{bucket.id()}",
                                h2 { class: "tk-social-notifs__day-title", {t(&format!("social.notifications.day.{}", bucket.id()))} }
                                ul {
                                    for (n , is_new) in list {
                                        NotificationRow { key: "{n.id}", notification: n.clone(), unread: is_new, now }
                                    }
                                }
                            }
                        }
                    }
                    if let Some(e) = more_error() {
                        ErrorState { error: e, compact: true, on_retry: move |_| load.call(page() + 1) }
                    } else if has_next() {
                        div { class: "tk-social__more",
                            Button {
                                label: if more_busy() { t("social.loading") } else { t("social.load_more") },
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: more_busy(),
                                onclick: move |_| load.call(page() + 1),
                            }
                        }
                    }
                },
            }
        }
    }
}

#[component]
fn NotificationRow(notification: NotificationVm, unread: bool, now: i64) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let n = notification;
    let target = target_of(&n);
    let open = {
        let backend = services.backend.clone();
        let target = target.clone();
        move |_| match target.clone() {
            NotificationTarget::Media(id) => {
                navigator.push(media_route(&id));
            }
            NotificationTarget::User(name) => {
                navigator.push(crate::app::Route::UserPage { name });
            }
            NotificationTarget::Web(url) => {
                let backend = backend.clone();
                spawn(async move {
                    if let Err(e) = backend.open_url(&url).await {
                        tracing::warn!(detail = %e.detail, "opening AniList failed");
                        snack.show(t("social.open_failed"));
                    }
                });
            }
            NotificationTarget::None => {}
        }
    };
    let clickable = target != NotificationTarget::None;
    rsx! {
        li {
            class: "tk-social-notif",
            "data-notification-id": "{n.id}",
            "data-notification-kind": "{n.kind}",
            "data-notification-group": "{notification_group(&n.kind)}",
            "data-unread": "{unread}",
            button {
                class: "tk-social-notif__btn",
                r#type: "button",
                disabled: !clickable,
                onclick: open,
                if let Some(m) = n.media.clone() {
                    span { class: "tk-social-notif__cover",
                        MediaCover { src: m.cover.clone(), color: m.color.clone(), title: m.title.clone() }
                    }
                } else if let Some(u) = n.user.clone() {
                    Avatar { user: u, size: 40 }
                }
                span { class: "tk-social-notif__text",
                    span { "{n.text}" }
                    span { class: "tk-muted tk-social-notif__time", {rel_time(now, n.created_unix)} }
                }
                if unread {
                    span { class: "tk-social-notif__dot", "aria-label": "{t(\"social.notifications.unread\")}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{MediaVm, UserVm};

    fn notif(id: u64, kind: &str, media: bool, user: bool, url: bool) -> NotificationVm {
        NotificationVm {
            id,
            kind: kind.into(),
            text: "x".into(),
            media: media.then(|| MediaVm { id: "21".into(), title: "One Piece".into(), cover: None, color: None, subtitle: String::new(), score: None }),
            user: user.then(|| UserVm { id: 7, name: "Mia".into(), avatar: None, url: String::new() }),
            created_unix: 0,
            url: url.then(|| "https://anilist.co/activity/1".to_owned()),
        }
    }

    #[test]
    fn targets_by_kind() {
        assert_eq!(target_of(&notif(1, "AIRING", true, false, true)), NotificationTarget::Media("21".into()));
        assert_eq!(target_of(&notif(2, "FOLLOWING", false, true, true)), NotificationTarget::User("Mia".into()));
        assert_eq!(
            target_of(&notif(3, "ACTIVITY_LIKE", false, true, true)),
            NotificationTarget::Web("https://anilist.co/activity/1".into())
        );
        assert_eq!(target_of(&notif(4, "ACTIVITY_LIKE", false, true, false)), NotificationTarget::User("Mia".into()));
        assert_eq!(target_of(&notif(5, "THREAD_LIKE", false, false, false)), NotificationTarget::None);
    }

    #[test]
    fn visible_rows_keep_unread_flags_by_position() {
        let items = vec![notif(1, "AIRING", true, false, false), notif(2, "FOLLOWING", false, true, false), notif(3, "AIRING", true, false, false)];
        let all = visible(&items, 2, "all");
        assert_eq!(all.iter().map(|(n, u)| (n.id, *u)).collect::<Vec<_>>(), vec![(1, true), (2, true), (3, false)]);
        let airing = visible(&items, 2, "airing");
        assert_eq!(airing.iter().map(|(n, u)| (n.id, *u)).collect::<Vec<_>>(), vec![(1, true), (3, false)]);
        assert!(visible(&items, 0, "forum").is_empty());
    }

    #[test]
    fn merge_skips_duplicates() {
        let mut items = vec![notif(1, "AIRING", false, false, false)];
        merge(&mut items, vec![notif(1, "AIRING", false, false, false), notif(2, "AIRING", false, false, false)]);
        assert_eq!(items.len(), 2);
    }
}
