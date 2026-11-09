//! "Add to list" button of the details page (wave A, worker **g**; docs/redesign/15-parity-wave-a-contract.md
//! section 4) and the other list widgets worker g owns: the favourite heart ([`FavoriteToggle`], shown next to the
//! list button, and [`CharacterFavorite`] for the character page) and the sync-conflict sheet ([`ConflictSheet`],
//! opened by the Library and Settings > Accounts).
//!
//! `ListButton` is mounted by worker c's `screens/details.rs` with these exact props. It loads
//! `Backend::list_entry(media_id)` itself, shows the current status (`lists.status.<id>`) or "Add to list", and
//! opens a menu with the six `ListStatusVm` statuses, progress -/+, score and "Remove from list". Writes go through
//! `Backend::set_list_entry` / `remove_list_entry` (local first + offline queue), so the button works without any
//! account (`lists.local-only`); then a hint points to Settings > Accounts. Verifier hooks on the root:
//! `data-list-status` (status id or `none`), `data-list-progress`, `data-list-pending`.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ConflictSideVm, ListAccountVm, ListConflictVm, ListEntryUpdateVm, ListEntryVm, ListStatusVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, LoadingIndicator, Menu, MenuItem,
    MenuSeparator, Sheet, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{ListServiceVm, UiError};

/// Prefix of character favourites in `Backend::{is_favorite, set_favorite}` (`character:<AniList id>`).
pub const CHARACTER_PREFIX: &str = "character:";

/// i18n key of a status.
pub fn status_key(s: ListStatusVm) -> String {
    format!("lists.status.{}", s.id())
}

/// Score scale the UI offers for `service`: MAL scores have 10 steps, AniList keeps 0..=100 (AniList converts it to
/// the account's own format: 10, 100, 5 stars or smileys). Returns `(max shown, step on the 0..=100 value)`.
pub fn score_scale(service: ListServiceVm) -> (u8, u8) {
    match service {
        ListServiceVm::Mal => (10, 10),
        ListServiceVm::AniList => (100, 1),
    }
}

/// `85` -> `"85/100"` (AniList) or `"9/10"` (MAL, rounded like the backend does).
pub fn score_text(score: u8, service: ListServiceVm) -> String {
    let (max, _) = score_scale(service);
    let shown = if max == 10 { (u16::from(score.min(100)) + 5) / 10 } else { u16::from(score.min(100)) };
    tf("lists.score_value", &[("n", &shown.to_string()), ("max", &max.to_string())])
}

/// `"3 / 12"`, or `"3"` when the episode count is unknown.
pub fn progress_text(progress: u32, total: Option<u32>) -> String {
    match total.filter(|t| *t > 0) {
        Some(total) => tf("lists.progress_value", &[("n", &progress.to_string()), ("total", &total.to_string())]),
        None => tf("lists.progress_value_open", &[("n", &progress.to_string())]),
    }
}

/// Next progress after a `delta` step, kept inside `0..=total`.
pub fn step_progress(progress: u32, delta: i32, total: Option<u32>) -> u32 {
    let next = if delta < 0 { progress.saturating_sub(delta.unsigned_abs()) } else { progress.saturating_add(delta as u32) };
    match total.filter(|t| *t > 0) {
        Some(t) => next.min(t),
        None => next,
    }
}

fn any_connected(accounts: &Option<Result<Vec<ListAccountVm>, UiError>>) -> bool {
    matches!(accounts, Some(Ok(list)) if list.iter().any(|a| a.connected))
}

#[component]
pub fn ListButton(
    /// `MediaVm::id` of the title.
    media_id: String,
    #[props(default)] episodes_total: Option<u32>,
    /// Icon-only variant for the floating toolbar.
    #[props(default)] compact: bool,
) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let navigator = use_navigator();
    let mut open = use_signal(|| false);
    let busy = use_signal(|| false);

    let mut entry = use_resource(use_reactive((&media_id,), {
        let backend = services.backend.clone();
        move |(id,)| {
            let backend = backend.clone();
            async move { backend.list_entry(&id).await }
        }
    }));
    let accounts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_accounts().await }
        }
    });

    let write = use_callback({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move |update: ListEntryUpdateVm| {
            let (mut busy, mut entry) = (busy, entry);
            if *busy.peek() {
                return;
            }
            busy.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.set_list_entry(&id, &update).await {
                    Ok(e) => {
                        if update.status.is_some() {
                            snack.show(tf("lists.saved", &[("status", &t(&status_key(e.status)))]));
                        }
                        entry.restart();
                    }
                    Err(e) => snack.show(format!("{} {}", t("lists.error_save"), t(e.kind.key()))),
                }
                busy.set(false);
            });
        }
    });
    let remove = use_callback({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move |_: ()| {
            let (mut busy, mut entry, mut open) = (busy, entry, open);
            if *busy.peek() {
                return;
            }
            busy.set(true);
            open.set(false);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.remove_list_entry(&id).await {
                    Ok(()) => {
                        snack.show(t("lists.removed"));
                        entry.restart();
                    }
                    Err(e) => snack.show(format!("{} {}", t("lists.error_save"), t(e.kind.key()))),
                }
                busy.set(false);
            });
        }
    });

    let state = entry.read().clone();
    let (current, failed): (Option<ListEntryVm>, Option<UiError>) = match state {
        Some(Ok(e)) => (e, None),
        Some(Err(e)) => (None, Some(e)),
        None => (None, None),
    };
    let loading = entry.read().is_none();
    let status_id = current.as_ref().map_or("none", |e| e.status.id());
    let progress = current.as_ref().map_or(0, |e| e.progress);
    let pending = current.as_ref().is_some_and(|e| e.pending);
    let total = current.as_ref().and_then(|e| e.episodes_total).or(episodes_total);
    let service = current.as_ref().map_or(ListServiceVm::AniList, |e| e.service);
    let connected = any_connected(&accounts.read());
    let label = match &current {
        Some(e) => t(&status_key(e.status)),
        None => t("lists.add"),
    };
    let icon = if current.is_some() { "check" } else { "bookmark" }.to_string();

    // A failed load: the button retries instead of opening the menu.
    if let Some(err) = failed {
        let retry_label = format!("{} - {}", t("lists.error_load"), t("common.retry"));
        return rsx! {
            span { class: "tk-listbtn", "data-list-status": "error", title: "{err.detail}",
                if compact {
                    IconButton { icon: "warning".to_string(), label: retry_label, onclick: move |_| entry.restart() }
                } else {
                    Button { label: retry_label, icon: "warning".to_string(), variant: ButtonVariant::Tonal, onclick: move |_| entry.restart() }
                }
                FavoriteToggle { media_id: media_id.clone() }
            }
        };
    }

    let trigger = if compact {
        rsx! {
            IconButton {
                icon: icon.clone(),
                label: label.clone(),
                toggled: current.is_some(),
                disabled: loading || busy(),
                onclick: move |_| open.toggle(),
            }
        }
    } else {
        rsx! {
            Button {
                label: label.clone(),
                icon: icon.clone(),
                trailing_icon: "chevron-down".to_string(),
                variant: if current.is_some() { ButtonVariant::Filled } else { ButtonVariant::Tonal },
                disabled: loading || busy(),
                onclick: move |_| open.toggle(),
            }
        }
    };

    let (score_max, score_step) = score_scale(service);
    let score_now = current.as_ref().and_then(|e| e.score).unwrap_or(0);
    let score_label = if score_now == 0 { t("lists.score_none") } else { score_text(score_now, service) };
    let has_entry = current.is_some();

    rsx! {
        span {
            class: "tk-listbtn",
            "data-list-status": "{status_id}",
            "data-list-progress": "{progress}",
            "data-list-pending": "{pending}",
            "aria-busy": if loading || busy() { "true" } else { "false" },
            Menu {
                open: open(),
                label: t("lists.menu_title"),
                title: t("lists.menu_title"),
                trigger,
                on_close: move |_| open.set(false),
                for s in ListStatusVm::ALL {
                    MenuItem {
                        key: "{s.id()}",
                        label: t(&status_key(s)),
                        checked: Some(current.as_ref().is_some_and(|e| e.status == s)),
                        disabled: busy(),
                        on_select: move |_| {
                            open.set(false);
                            write.call(ListEntryUpdateVm { status: Some(s), ..ListEntryUpdateVm::default() });
                        },
                    }
                }
                if has_entry {
                    MenuSeparator {}
                    div { class: "tk-listbtn__row",
                        span { class: "tk-listbtn__label", {t("lists.progress")} }
                        IconButton {
                            icon: "minus".to_string(),
                            label: t("lists.progress_dec"),
                            disabled: busy() || progress == 0,
                            onclick: move |_| write.call(ListEntryUpdateVm { progress: Some(step_progress(progress, -1, total)), ..ListEntryUpdateVm::default() }),
                        }
                        span { class: "tk-listbtn__value", "data-list-progress": "{progress}", {progress_text(progress, total)} }
                        IconButton {
                            icon: "plus".to_string(),
                            label: t("lists.progress_inc"),
                            disabled: busy() || total.is_some_and(|t| t > 0 && progress >= t),
                            onclick: move |_| write.call(ListEntryUpdateVm { progress: Some(step_progress(progress, 1, total)), ..ListEntryUpdateVm::default() }),
                        }
                    }
                    div { class: "tk-listbtn__row",
                        label { class: "tk-listbtn__label", r#for: "tk-listbtn-score", {t("lists.score")} }
                        input {
                            id: "tk-listbtn-score",
                            class: "tk-listbtn__range",
                            r#type: "range",
                            min: "0",
                            max: "100",
                            step: "{score_step}",
                            value: "{score_now}",
                            disabled: busy(),
                            "aria-valuetext": "{score_label}",
                            "data-score-max": "{score_max}",
                            onchange: move |e| {
                                if let Ok(v) = e.value().parse::<u8>() {
                                    write.call(ListEntryUpdateVm { score: Some(v.min(100)), ..ListEntryUpdateVm::default() });
                                }
                            },
                        }
                        span { class: "tk-listbtn__value", "{score_label}" }
                    }
                    MenuSeparator {}
                    MenuItem { label: t("lists.remove"), icon: "trash".to_string(), disabled: busy(), on_select: move |_| remove.call(()) }
                }
                if !connected {
                    MenuSeparator {}
                    p { class: "tk-listbtn__hint", {t("lists.local_hint")} }
                    MenuItem {
                        label: t("lists.connect"),
                        icon: "user".to_string(),
                        on_select: move |_| {
                            open.set(false);
                            navigator.push(Route::SettingsPage {});
                        },
                    }
                }
            }
            if pending {
                span { class: "tk-listbtn__pending", title: t("lists.pending"), "aria-label": t("lists.pending"),
                    Icon { name: "sync".to_string(), size: 16 }
                }
            }
            FavoriteToggle { media_id: media_id.clone() }
        }
    }
}

/// Heart toggle for a title or a character (`character:<id>`): local favourite + AniList `ToggleFavourite` when an
/// AniList account is connected (queued offline). `data-favorite` = `true | false | loading | error`.
#[component]
pub fn FavoriteToggle(media_id: String) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let busy = use_signal(|| false);
    let mut fav = use_resource(use_reactive((&media_id,), {
        let backend = services.backend.clone();
        move |(id,)| {
            let backend = backend.clone();
            async move { backend.is_favorite(&id).await }
        }
    }));
    let toggle = use_callback({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move |next: bool| {
            let (mut busy, mut fav) = (busy, fav);
            if *busy.peek() {
                return;
            }
            busy.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.set_favorite(&id, next).await {
                    Ok(()) => snack.show(t(if next { "lists.fav.added" } else { "lists.fav.removed" })),
                    Err(e) => snack.show(format!("{} {}", t("lists.error_save"), t(e.kind.key()))),
                }
                fav.restart();
                busy.set(false);
            });
        }
    });
    let state = match &*fav.read() {
        None => "loading",
        Some(Err(_)) => "error",
        Some(Ok(true)) => "true",
        Some(Ok(false)) => "false",
    };
    let on = state == "true";
    let label = if on { t("lists.fav.remove") } else { t("lists.fav.add") };
    let icon = if on { "heart-filled" } else { "heart" }.to_string();
    rsx! {
        span { class: "tk-fav", "data-favorite": "{state}",
            IconButton {
                icon,
                label,
                toggle: true,
                toggled: on,
                disabled: busy() || state == "loading",
                onclick: move |_| {
                    if state == "error" {
                        fav.restart();
                    } else {
                        toggle.call(!on);
                    }
                },
            }
        }
    }
}

/// Character page heart (`character.favorite`): fills the `data-favorite-slot` worker a leaves in
/// `screens/character.rs`. Saves `character:<id>` locally (Favourites tab) and on AniList when connected; without an
/// AniList account it invites the user to connect one.
#[component]
pub fn CharacterFavorite(character_id: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let accounts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_accounts().await }
        }
    });
    let anilist = matches!(
        &*accounts.read(),
        Some(Ok(list)) if list.iter().any(|a| a.service == ListServiceVm::AniList && a.connected)
    );
    let loaded = accounts.read().is_some();
    rsx! {
        div { class: "tk-charfav", "data-anilist": "{anilist}",
            FavoriteToggle { media_id: format!("{CHARACTER_PREFIX}{character_id}") }
            if loaded && !anilist {
                p { class: "tk-charfav__hint",
                    {t("lists.fav.char_hint")}
                    Button {
                        label: t("lists.connect"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::SettingsPage {});
                        },
                    }
                }
            }
        }
    }
}

/// One side of a conflict, compact.
#[component]
fn ConflictSide(title: String, entry: ListEntryVm) -> Element {
    let status = t(&status_key(entry.status));
    let progress = progress_text(entry.progress, entry.episodes_total);
    let score = match entry.score {
        Some(s) if s > 0 => score_text(s, entry.service),
        _ => t("lists.score_none"),
    };
    rsx! {
        div { class: "tk-conflict__side",
            div { class: "tk-conflict__who", "{title}" }
            div { class: "tk-conflict__status", "data-status": "{entry.status.id()}", "{status}" }
            div { class: "tk-muted", "{progress} · {score}" }
        }
    }
}

/// Sheet listing every open conflict with "keep this PC" / "keep <service>" per entry and for all
/// (`lists.conflicts`). `on_close` fires when the user closes it; `on_changed` after each resolution.
#[component]
pub fn ConflictSheet(on_close: EventHandler<()>, #[props(default)] on_changed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let busy = use_signal(|| false);
    let mut list = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_conflicts().await }
        }
    });
    let resolve = use_callback({
        let backend = services.backend.clone();
        move |(ids, side): (Vec<i64>, ConflictSideVm)| {
            let (mut busy, mut list) = (busy, list);
            if *busy.peek() {
                return;
            }
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                let mut failed = None;
                for id in ids {
                    if let Err(e) = backend.resolve_list_conflict(id, side).await {
                        failed = Some(e);
                    }
                }
                match failed {
                    Some(e) => snack.show(format!("{} {}", t("lists.error_save"), t(e.kind.key()))),
                    None => snack.show(t("lists.conflicts.resolved")),
                }
                list.restart();
                on_changed.call(());
                busy.set(false);
            });
        }
    });

    let body = match &*list.read() {
        None => rsx! { LoadingIndicator {} },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| list.restart() }
        },
        Some(Ok(items)) if items.is_empty() => rsx! {
            EmptyState { icon: "check".to_string(), title: t("lists.conflicts.empty") }
        },
        Some(Ok(items)) => {
            let all: Vec<i64> = items.iter().map(|c| c.id).collect();
            let all2 = all.clone();
            let n = items.len();
            rsx! {
                p { class: "tk-panel__hint", {tn("lists.conflicts.count", n)} }
                div { class: "tk-conflict__all",
                    Button {
                        label: t("lists.conflicts.all_local"),
                        variant: ButtonVariant::Tonal,
                        disabled: busy(),
                        onclick: move |_| resolve.call((all.clone(), ConflictSideVm::Local)),
                    }
                    Button {
                        label: t("lists.conflicts.all_remote"),
                        variant: ButtonVariant::Tonal,
                        disabled: busy(),
                        onclick: move |_| resolve.call((all2.clone(), ConflictSideVm::Remote)),
                    }
                }
                ul { class: "tk-conflict__list",
                    for c in items.clone() {
                        {
                            let id = c.id;
                            rsx! {
                                ConflictRow {
                                    key: "{id}",
                                    conflict: c,
                                    busy: busy(),
                                    on_resolve: move |side: ConflictSideVm| resolve.call((vec![id], side)),
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    let count = match &*list.read() {
        Some(Ok(items)) => items.len(),
        _ => 0,
    };
    rsx! {
        Sheet { title: t("lists.conflicts.title"), wide: true, on_close: move |_| on_close.call(()),
            div { class: "tk-conflicts", "data-conflicts": "{count}",
                {body}
            }
        }
    }
}

#[component]
fn ConflictRow(conflict: ListConflictVm, busy: bool, on_resolve: EventHandler<ConflictSideVm>) -> Element {
    let service = conflict.remote.service.label().to_string();
    rsx! {
        li { class: "tk-conflict", "data-conflict-id": "{conflict.id}",
            div { class: "tk-conflict__title", "{conflict.local.media.title}" }
            div { class: "tk-conflict__sides",
                ConflictSide { title: t("lists.conflicts.local"), entry: conflict.local.clone() }
                ConflictSide { title: service.clone(), entry: conflict.remote.clone() }
            }
            div { class: "tk-conflict__actions",
                Button {
                    label: t("lists.conflicts.keep_local"),
                    variant: ButtonVariant::Outlined,
                    size: ButtonSize::Sm,
                    disabled: busy,
                    onclick: move |_| on_resolve.call(ConflictSideVm::Local),
                }
                Button {
                    label: tf("lists.conflicts.keep_remote", &[("service", &service)]),
                    variant: ButtonVariant::Outlined,
                    size: ButtonSize::Sm,
                    disabled: busy,
                    onclick: move |_| on_resolve.call(ConflictSideVm::Remote),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_steps_stay_in_range() {
        assert_eq!(step_progress(0, -1, Some(12)), 0);
        assert_eq!(step_progress(3, 1, Some(12)), 4);
        assert_eq!(step_progress(12, 1, Some(12)), 12);
        assert_eq!(step_progress(40, 1, None), 41);
        assert_eq!(step_progress(5, 1, Some(0)), 6, "0 episodes = unknown");
    }

    #[test]
    fn score_scales_follow_the_service() {
        assert_eq!(score_scale(ListServiceVm::Mal), (10, 10));
        assert_eq!(score_scale(ListServiceVm::AniList), (100, 1));
    }

    #[test]
    fn status_keys() {
        assert_eq!(status_key(ListStatusVm::Paused), "lists.status.paused");
    }
}
