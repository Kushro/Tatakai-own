//! AniList activity feed `/activity` (worker **n**; checklist `social.activity.feed`, `social.activity.interact`;
//! classic `lib/anilist/client.ts` activity queries): tabs Following · Global (`Backend::anilist_activity(feed, page)`,
//! `data-feed`), like (`anilist_like_activity`, `data-liked`), replies thread (`anilist_activity_replies`,
//! `anilist_reply`), "Post a status" (`anilist_post_status`, the new entry appears on top), delete own
//! (`anilist_delete_activity`). [`ActivityList`] and [`ActivityCard`] are reused by the profile page (the user's
//! activity) and by `components::Reviews` (the viewer's activity on a title).

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ActivityFeedVm, ActivityKindVm, ActivityReplyVm, ActivityVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize, MediaCover,
    SegOption, Segmented, SkeletonLines, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::model::UiError;
use crate::model::social::wake_unread_poll;
use crate::platform::now_unix;
use crate::screens::social::{Avatar, Disconnected, SocialTabs, is_auth, media_route, paragraphs, rel_time};

/// AniList page size of `Page.activities` (`tk_core::metadata::anilist::social::ACTIVITIES_PER_PAGE`): a shorter page
/// is the last one.
pub const ACTIVITY_PAGE_SIZE: usize = 25;
/// AniList's limit for text activities and replies.
pub const MAX_TEXT_CHARS: usize = 2000;

/// i18n key of AniList's list-activity status (`watched episode`, `plans to read` ...): `social.activity.status.<snake>`.
pub fn status_key(status: &str) -> String {
    let snake: String = status
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    format!("social.activity.status.{snake}")
}

/// The status in the UI language, AniList's own words when the status is unknown.
pub fn status_text(status: &str) -> String {
    let key = status_key(status);
    let s = crate::i18n::lookup(crate::i18n::lang(), &key);
    if s == key { status.trim().to_owned() } else { s }
}

/// Text that can be posted: trimmed, not empty, within AniList's limit.
pub fn postable(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty() && t.chars().count() <= MAX_TEXT_CHARS
}

/// `items` + `page` merged without duplicates (a new status posted meanwhile shifts AniList's pages).
pub fn merge_page(items: &mut Vec<ActivityVm>, page: Vec<ActivityVm>) {
    for a in page {
        if !items.iter().any(|x| x.id == a.id) {
            items.push(a);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Load {
    Loading,
    Ready { has_next: bool },
    Failed(UiError),
}

#[component]
pub fn ActivityPage() -> Element {
    let mut feed = use_signal(|| "following".to_string());
    let options: Vec<SegOption> = vec![
        ("following".to_string(), t("social.activity.feed.following")),
        ("global".to_string(), t("social.activity.feed.global")),
    ];
    let current = feed();
    let vm = if current == "global" { ActivityFeedVm::Global } else { ActivityFeedVm::Following };
    rsx! {
        div { class: "tk-page tk-social", "data-page": "social.activity.title", "data-feed": "{current}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("social.activity.title")} }
                    SocialTabs { current: "activity".to_string() }
                }
                Segmented {
                    options,
                    selected: current.clone(),
                    label: t("social.activity.feed.label"),
                    on_select: move |id: String| feed.set(id),
                }
            }
            // Keyed: switching tabs remounts the list (fresh pages).
            {std::iter::once(current.clone()).map(|k| rsx! {
                ActivityList { key: "{k}", feed: vm.clone(), composer: true }
            })}
        }
    }
}

/// A paginated activity feed with an optional "Post a status" box.
#[component]
pub fn ActivityList(
    feed: ActivityFeedVm,
    #[props(default)] composer: bool,
    /// Compact empty / disconnected states (inside Details or a profile tab).
    #[props(default)] compact: bool,
) -> Element {
    let services = use_context::<Services>();
    let mut items = use_signal(Vec::<ActivityVm>::new);
    let mut state = use_signal(|| Load::Loading);
    let mut page = use_signal(|| 1_u32);
    let mut more_busy = use_signal(|| false);
    let mut reload = use_signal(|| 0_u32);
    // The viewer's id (delete own entries); errors mean "not connected" and simply hide Delete.
    let me = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.anilist_profile(None).await.ok().map(|p| p.user.id) }
        }
    });
    let me_id: Option<u64> = me.read().clone().flatten();

    // First page (again on Retry).
    use_effect({
        let backend = services.backend.clone();
        let feed = feed.clone();
        move || {
            let _ = reload();
            let (backend, feed) = (backend.clone(), feed.clone());
            spawn(async move {
                state.set(Load::Loading);
                match backend.anilist_activity(&feed, 1).await {
                    Ok(list) => {
                        let has_next = list.len() >= ACTIVITY_PAGE_SIZE;
                        items.set(list);
                        page.set(1);
                        state.set(Load::Ready { has_next });
                    }
                    Err(e) => state.set(Load::Failed(e)),
                }
            });
        }
    });

    let load_more = use_callback({
        let backend = services.backend.clone();
        let feed = feed.clone();
        move |()| {
            if *more_busy.peek() {
                return;
            }
            let (backend, feed) = (backend.clone(), feed.clone());
            let next = *page.peek() + 1;
            more_busy.set(true);
            spawn(async move {
                match backend.anilist_activity(&feed, next).await {
                    Ok(list) => {
                        let has_next = list.len() >= ACTIVITY_PAGE_SIZE;
                        merge_page(&mut items.write(), list);
                        page.set(next);
                        state.set(Load::Ready { has_next });
                    }
                    Err(e) => state.set(Load::Failed(e)),
                }
                more_busy.set(false);
            });
        }
    });

    let current = state.read().clone();
    let list = items.read().clone();
    let count = list.len();
    let feed_id = match &feed {
        ActivityFeedVm::Following => "following".to_string(),
        ActivityFeedVm::Global => "global".to_string(),
        ActivityFeedVm::User(id) => format!("user:{id}"),
        ActivityFeedVm::Media(id) => format!("media:{id}"),
    };

    rsx! {
        div { class: "tk-social-feed", "data-feed": "{feed_id}", "data-count": "{count}",
            if composer && !matches!(current, Load::Failed(ref e) if is_auth(e)) {
                StatusComposer {
                    on_posted: move |a: ActivityVm| {
                        items.write().insert(0, a);
                    },
                }
            }
            match current {
                Load::Loading if list.is_empty() => rsx! {
                    div { "aria-busy": "true", "data-loading": "true",
                        SkeletonLines { widths: vec![90, 70, 85, 60] }
                    }
                },
                Load::Failed(e) if is_auth(&e) => rsx! {
                    Disconnected { compact }
                },
                Load::Failed(e) if list.is_empty() => rsx! {
                    ErrorState { error: e, compact, on_retry: move |_| reload += 1 }
                },
                _ if list.is_empty() => rsx! {
                    if compact {
                        p { class: "tk-muted", "data-empty": "true", {t("social.activity.empty.body")} }
                    } else {
                        EmptyState { icon: "history".to_string(), title: t("social.activity.empty.title"), body: t("social.activity.empty.body") }
                    }
                },
                other => rsx! {
                    ul { class: "tk-social-feed__list",
                        for a in list.clone() {
                            ActivityCard {
                                key: "{a.id}",
                                activity: a.clone(),
                                me: me_id,
                                on_deleted: move |id: u64| {
                                    items.write().retain(|x| x.id != id);
                                },
                            }
                        }
                    }
                    if let Load::Failed(e) = other.clone() {
                        ErrorState { error: e, compact: true, on_retry: move |_| load_more.call(()) }
                    } else if matches!(other, Load::Ready { has_next: true }) {
                        div { class: "tk-social__more",
                            Button {
                                label: if more_busy() { t("social.loading") } else { t("social.load_more") },
                                variant: ButtonVariant::Tonal,
                                disabled: more_busy(),
                                onclick: move |_| load_more.call(()),
                            }
                        }
                    }
                },
            }
        }
    }
}

/// "Post a status" box (`SaveTextActivity`).
#[component]
fn StatusComposer(on_posted: EventHandler<ActivityVm>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut text = use_signal(String::new);
    let mut generation = use_signal(|| 0_u32);
    let mut busy = use_signal(|| false);
    let ok = postable(&text());
    let post = {
        let backend = services.backend.clone();
        move |_| {
            let body = text();
            if busy() || !postable(&body) {
                return;
            }
            let backend = backend.clone();
            busy.set(true);
            spawn(async move {
                match backend.anilist_post_status(&body).await {
                    Ok(a) => {
                        text.set(String::new());
                        generation += 1;
                        on_posted.call(a);
                        snack.show(t("social.activity.posted"));
                    }
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "posting an AniList status failed");
                        snack.show(t("social.activity.post_failed"));
                    }
                }
                busy.set(false);
            });
        }
    };
    let chars = text().trim().chars().count();
    rsx! {
        div { class: "tk-social-compose", "data-composer": "status",
            {std::iter::once(generation()).map(|g| rsx! {
                textarea {
                    key: "{g}",
                    class: "tk-social-compose__input",
                    rows: "3",
                    maxlength: "{MAX_TEXT_CHARS}",
                    placeholder: "{t(\"social.activity.compose.placeholder\")}",
                    "aria-label": "{t(\"social.activity.compose.label\")}",
                    oninput: move |e| text.set(e.value()),
                }
            })}
            div { class: "tk-social-compose__bar",
                span { class: "tk-muted", "{chars}/{MAX_TEXT_CHARS}" }
                Button {
                    label: t("social.activity.compose.post"),
                    icon: "upload".to_string(),
                    variant: ButtonVariant::Filled,
                    size: ButtonSize::Sm,
                    disabled: !ok || busy(),
                    onclick: post,
                }
            }
        }
    }
}

/// One activity: author, what happened, like / replies / delete / open on AniList.
#[component]
pub fn ActivityCard(activity: ActivityVm, me: Option<u64>, on_deleted: EventHandler<u64>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let a = activity;
    let mut liked = use_signal(|| a.liked);
    let mut likes = use_signal(|| a.likes);
    let mut reply_count = use_signal(|| a.replies);
    let mut open = use_signal(|| false);
    let mut confirm = use_signal(|| false);
    let mut like_busy = use_signal(|| false);
    let own = me == Some(a.user.id);
    let id = a.id;
    let now = now_unix();

    let toggle_like = {
        let backend = services.backend.clone();
        move |_| {
            if like_busy() {
                return;
            }
            let want = !liked();
            let backend = backend.clone();
            // Optimistic; reverted on failure.
            liked.set(want);
            let before = likes();
            likes.set(if want { before + 1 } else { before.saturating_sub(1) });
            like_busy.set(true);
            spawn(async move {
                if let Err(e) = backend.anilist_like_activity(id, want).await {
                    tracing::warn!(detail = %e.detail, "AniList like failed");
                    liked.set(!want);
                    likes.set(before);
                    snack.show(if is_auth(&e) { t("social.disconnected.body") } else { t("social.activity.like_failed") });
                }
                like_busy.set(false);
            });
        }
    };
    let delete = {
        let backend = services.backend.clone();
        move |_| {
            let backend = backend.clone();
            confirm.set(false);
            spawn(async move {
                match backend.anilist_delete_activity(id).await {
                    Ok(()) => {
                        on_deleted.call(id);
                        snack.show(t("social.activity.deleted"));
                    }
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "deleting an AniList activity failed");
                        snack.show(t("social.activity.delete_failed"));
                    }
                }
            });
        }
    };
    let open_url = {
        let backend = services.backend.clone();
        let url = a.url.clone();
        move |_| {
            let (backend, url) = (backend.clone(), url.clone());
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening AniList failed");
                    snack.show(t("social.open_failed"));
                }
            });
        }
    };

    let user_name = a.user.name.clone();
    let like_label = if liked() { t("social.activity.unlike") } else { t("social.activity.like") };
    let replies_label = tn("social.activity.replies", reply_count() as usize);
    let kind = a.kind.id();

    rsx! {
        li { class: "tk-social-card", "data-activity-id": "{id}", "data-activity-kind": "{kind}", "data-liked": "{liked()}",
            div { class: "tk-social-card__head",
                button {
                    class: "tk-social-card__user",
                    r#type: "button",
                    onclick: move |_| {
                        navigator.push(Route::UserPage { name: user_name.clone() });
                    },
                    Avatar { user: a.user.clone(), size: 36 }
                    b { "{a.user.name}" }
                }
                span { class: "tk-muted tk-social-card__time", {rel_time(now, a.created_unix)} }
            }
            div { class: "tk-social-card__body",
                if a.kind == ActivityKindVm::List {
                    ListLine { activity: a.clone() }
                } else if let Some(text) = a.text.clone() {
                    div { class: "tk-social-card__text",
                        for (i , p) in paragraphs(&text).into_iter().enumerate() {
                            p { key: "{i}", "{p}" }
                        }
                    }
                }
            }
            div { class: "tk-social-card__actions",
                IconButton {
                    icon: if liked() { "heart-filled".to_string() } else { "heart".to_string() },
                    label: like_label,
                    size: IconButtonSize::Sm,
                    toggle: true,
                    toggled: liked(),
                    onclick: toggle_like,
                }
                span { class: "tk-social-card__count", "data-likes": "{likes()}", "{likes()}" }
                Button {
                    label: replies_label,
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    icon: if open() { "chevron-up".to_string() } else { "chevron-down".to_string() },
                    onclick: move |_| {
                        let next = !open();
                        open.set(next);
                    },
                }
                span { class: "tk-social-card__spacer" }
                if own {
                    IconButton {
                        icon: "trash".to_string(),
                        label: t("social.activity.delete"),
                        size: IconButtonSize::Sm,
                        onclick: move |_| confirm.set(true),
                    }
                }
                IconButton { icon: "external".to_string(), label: t("social.open_anilist"), size: IconButtonSize::Sm, onclick: open_url }
            }
            if open() {
                Replies {
                    activity_id: id,
                    on_replied: move |_| {
                        reply_count += 1;
                    },
                }
            }
            if confirm() {
                ConfirmDialog {
                    title: t("social.activity.delete_title"),
                    body: t("social.activity.delete_body"),
                    confirm_label: t("social.activity.delete"),
                    icon: "trash".to_string(),
                    on_confirm: delete,
                    on_cancel: move |_| confirm.set(false),
                }
            }
        }
    }
}

/// "Watched episode 5 of <title>" with the cover.
#[component]
fn ListLine(activity: ActivityVm) -> Element {
    let navigator = use_navigator();
    let a = activity;
    let status = a.status.as_deref().map(status_text).unwrap_or_default();
    let line = match a.progress.clone() {
        Some(p) => tf("social.activity.line.progress", &[("status", &status), ("progress", &p)]),
        None => status.clone(),
    };
    rsx! {
        div { class: "tk-social-card__list",
            if let Some(m) = a.media.clone() {
                {
                    let target = media_route(&m.id);
                    let target2 = target.clone();
                    rsx! {
                        button {
                            class: "tk-social-card__cover",
                            r#type: "button",
                            "aria-label": "{m.title}",
                            onclick: move |_| {
                                navigator.push(target.clone());
                            },
                            MediaCover { src: m.cover.clone(), color: m.color.clone(), title: m.title.clone() }
                        }
                        p { class: "tk-social-card__line",
                            span { "{line} " }
                            button {
                                class: "tk-social-card__title",
                                r#type: "button",
                                "data-media-id": "{m.id}",
                                onclick: move |_| {
                                    navigator.push(target2.clone());
                                },
                                "{m.title}"
                            }
                        }
                    }
                }
            } else {
                p { class: "tk-social-card__line", "{line}" }
            }
        }
    }
}

/// The reply thread of an activity, with a reply box.
#[component]
fn Replies(activity_id: u64, on_replied: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let mut replies = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.anilist_activity_replies(activity_id).await }
        }
    });
    let mut added = use_signal(Vec::<ActivityReplyVm>::new);
    let mut text = use_signal(String::new);
    let mut generation = use_signal(|| 0_u32);
    let mut busy = use_signal(|| false);
    let send = {
        let backend = services.backend.clone();
        move |_| {
            let body = text();
            if busy() || !postable(&body) {
                return;
            }
            let backend = backend.clone();
            busy.set(true);
            spawn(async move {
                match backend.anilist_reply(activity_id, &body).await {
                    Ok(r) => {
                        added.write().push(r);
                        text.set(String::new());
                        generation += 1;
                        on_replied.call(());
                        // A reply can come with notifications for the viewer (subscriptions): refresh the badge.
                        wake_unread_poll();
                    }
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "AniList reply failed");
                        snack.show(if is_auth(&e) { t("social.disconnected.body") } else { t("social.activity.reply_failed") });
                    }
                }
                busy.set(false);
            });
        }
    };
    let now = now_unix();
    let state = replies.read().clone();
    let extra = added.read().clone();
    let ok = postable(&text());
    rsx! {
        div { class: "tk-social-replies", "data-replies-for": "{activity_id}",
            match state {
                None => rsx! { SkeletonLines { widths: vec![70, 50] } },
                Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| replies.restart() } },
                Some(Ok(list)) => {
                    let mut all = list;
                    for r in extra {
                        if !all.iter().any(|x| x.id == r.id) {
                            all.push(r);
                        }
                    }
                    rsx! {
                        if all.is_empty() {
                            p { class: "tk-muted", {t("social.activity.no_replies")} }
                        }
                        ul { class: "tk-social-replies__list", "data-replies": "{all.len()}",
                            for r in all.iter().cloned() {
                                {
                                    let name = r.user.name.clone();
                                    rsx! {
                                        li { key: "{r.id}", class: "tk-social-reply", "data-reply-id": "{r.id}",
                                            button {
                                                class: "tk-social-card__user",
                                                r#type: "button",
                                                onclick: move |_| {
                                                    navigator.push(Route::UserPage { name: name.clone() });
                                                },
                                                Avatar { user: r.user.clone(), size: 28 }
                                                b { "{r.user.name}" }
                                            }
                                            span { class: "tk-muted tk-social-card__time", {rel_time(now, r.created_unix)} }
                                            div { class: "tk-social-reply__text",
                                                for (i , p) in paragraphs(&r.text).into_iter().enumerate() {
                                                    p { key: "{i}", "{p}" }
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
            div { class: "tk-social-compose tk-social-compose--reply", "data-composer": "reply",
                {std::iter::once(generation()).map(|g| rsx! {
                    textarea {
                        key: "{g}",
                        class: "tk-social-compose__input",
                        rows: "2",
                        maxlength: "{MAX_TEXT_CHARS}",
                        placeholder: "{t(\"social.activity.reply.placeholder\")}",
                        "aria-label": "{t(\"social.activity.reply.label\")}",
                        oninput: move |e| text.set(e.value()),
                    }
                })}
                div { class: "tk-social-compose__bar",
                    Button {
                        label: t("social.activity.reply.send"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: !ok || busy(),
                        onclick: send,
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::UserVm;

    fn act(id: u64) -> ActivityVm {
        ActivityVm {
            id,
            kind: ActivityKindVm::Text,
            user: UserVm { id: 1, name: "a".into(), avatar: None, url: String::new() },
            media: None,
            status: None,
            progress: None,
            text: Some("x".into()),
            created_unix: 0,
            likes: 0,
            liked: false,
            replies: 0,
            url: String::new(),
        }
    }

    #[test]
    fn status_keys_are_snake_case() {
        assert_eq!(status_key("watched episode"), "social.activity.status.watched_episode");
        assert_eq!(status_key(" Plans to read "), "social.activity.status.plans_to_read");
        assert!(!status_text("watched episode").contains("social."));
        assert_eq!(status_text("something new"), "something new");
    }

    #[test]
    fn postable_text() {
        assert!(!postable("   "));
        assert!(postable(" hi "));
        assert!(!postable(&"x".repeat(MAX_TEXT_CHARS + 1)));
    }

    #[test]
    fn pages_merge_without_duplicates() {
        let mut items = vec![act(3), act(2)];
        merge_page(&mut items, vec![act(2), act(1)]);
        assert_eq!(items.iter().map(|a| a.id).collect::<Vec<_>>(), vec![3, 2, 1]);
    }
}
