//! Library (wave A, worker **g**; docs/redesign/15 section 4, `lists.library-tabs`): tabs Continue · History ·
//! Watching · Planning · Completed · Paused · Dropped · Repeating · Favourites. Continue and History come from
//! `Backend::library()` (local, history syncs with the phone over Tatakai Link, plan 14 section 0.1 #9); the status
//! tabs read the local list mirror (`Backend::list_entries`, works without an account) with progress -/+ written
//! through `Backend::set_list_entry`; Favourites lists titles and characters (`Backend::favorites`).
//!
//! Compact windows (phone layout) show Continue · History · Lists · Favourites · Torrents, with the statuses as a
//! segmented control inside Lists. Every list tab sorts by last update, title, score or progress and filters by
//! title (`lists.library-sort-filter`). "Sync lists" calls `Backend::list_sync_now`: success and "no account
//! connected" (`ErrorKind::Auth`) confirm in the snackbar, other failures stay inline; open conflicts show a
//! banner that opens the `ConflictSheet`. Verifier hooks: `[role="tab"][data-status]`, `data-list-status` /
//! `data-list-progress` on each entry row.

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{EpisodeVm, HistoryItemVm, LibraryVm, ListEntryUpdateVm, ListEntryVm, ListStatusVm, MediaVm};
use crate::components::list_button::{CHARACTER_PREFIX, ConflictSheet, progress_text, score_text, status_key, step_progress};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, EmptyState, EpisodeRow, ErrorState, IconButton, IconButtonSize,
    MediaCard, MediaCover, Pill, SegOption, Segmented, SkeletonGrid, SkeletonLines, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{ErrorKind, UiError, WindowClass, format_time, window_class};
use crate::platform::now_unix;

/// Relative time as `(i18n key, n)` (prototype `ui.js::relTime`); `n` fills `{n}` when present.
fn rel_time_parts(elapsed_secs: i64) -> (&'static str, Option<i64>) {
    let m = elapsed_secs.abs() / 60;
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

fn rel_time(now_unix: i64, then_unix: i64) -> String {
    match rel_time_parts(now_unix - then_unix) {
        (key, Some(n)) => tf(key, &[("n", &n.to_string())]),
        (key, None) => t(key),
    }
}

/// The Library tabs. `List(status)` is one status tab (expanded) or the Lists tab with that status picked (compact).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibTab {
    Continue,
    History,
    List(ListStatusVm),
    Favorites,
}

impl LibTab {
    /// `data-status` value and i18n suffix (`lib.tab.<id>` / `lists.status.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::History => "history",
            Self::List(s) => s.id(),
            Self::Favorites => "favorites",
        }
    }

    /// The nine tabs of an expanded window, in order.
    pub fn expanded() -> Vec<LibTab> {
        let mut v = vec![Self::Continue, Self::History];
        v.extend(ListStatusVm::ALL.into_iter().map(Self::List));
        v.push(Self::Favorites);
        v
    }

    fn label(self) -> String {
        match self {
            Self::List(s) => t(&status_key(s)),
            other => t(&format!("lib.tab.{}", other.id())),
        }
    }
}

/// Sort order of a list tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibSort {
    #[default]
    Updated,
    Title,
    Score,
    Progress,
}

impl LibSort {
    pub const ALL: [LibSort; 4] = [Self::Updated, Self::Title, Self::Score, Self::Progress];

    pub fn id(self) -> &'static str {
        match self {
            Self::Updated => "updated",
            Self::Title => "title",
            Self::Score => "score",
            Self::Progress => "progress",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL.into_iter().find(|s| s.id() == id).unwrap_or_default()
    }
}

/// Case-insensitive title filter (empty query keeps everything).
pub fn matches_query(title: &str, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || title.to_lowercase().contains(&q)
}

/// Filter + sort list entries: last update and score / progress descending, title ascending; ties by title.
pub fn sort_entries(mut entries: Vec<ListEntryVm>, sort: LibSort, query: &str) -> Vec<ListEntryVm> {
    entries.retain(|e| matches_query(&e.media.title, query));
    let title = |e: &ListEntryVm| e.media.title.to_lowercase();
    entries.sort_by(|a, b| {
        let primary = match sort {
            LibSort::Updated => b.updated_unix.cmp(&a.updated_unix),
            LibSort::Title => std::cmp::Ordering::Equal,
            LibSort::Score => b.score.unwrap_or(0).cmp(&a.score.unwrap_or(0)),
            LibSort::Progress => b.progress.cmp(&a.progress),
        };
        primary.then_with(|| title(a).cmp(&title(b)))
    });
    entries
}

/// Filter + sort favourites (`Updated` keeps the backend's newest-first order).
pub fn sort_media(mut items: Vec<MediaVm>, sort: LibSort, query: &str) -> Vec<MediaVm> {
    items.retain(|m| matches_query(&m.title, query));
    match sort {
        LibSort::Title => items.sort_by_key(|m| m.title.to_lowercase()),
        LibSort::Score => items.sort_by(|a, b| b.score.unwrap_or(0).cmp(&a.score.unwrap_or(0))),
        LibSort::Updated | LibSort::Progress => {}
    }
    items
}

#[component]
pub fn LibraryPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut lib = use_resource(move || {
        let backend = backend.clone();
        async move { backend.library().await }
    });
    let mut conflicts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_conflicts().await }
        }
    });
    let accounts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_accounts().await }
        }
    });
    let mut tab = use_signal(|| LibTab::Continue);
    let mut last_status = use_signal(|| ListStatusVm::Watching);
    let mut sort = use_signal(LibSort::default);
    let mut query = use_signal(String::new);
    // Bumped after a sync or a conflict resolution so the list tabs reload.
    let mut rev = use_signal(|| 0_u32);
    let mut conflicts_open = use_signal(|| false);
    let syncing = use_signal(|| false);
    let sync_error = use_signal(|| None::<UiError>);

    let sync_backend = services.backend.clone();
    let sync = use_callback(move |_: ()| {
        if *syncing.peek() {
            return;
        }
        let backend = sync_backend.clone();
        let (mut syncing, mut sync_error, mut lib, mut conflicts, mut rev, mut conflicts_open) =
            (syncing, sync_error, lib, conflicts, rev, conflicts_open);
        syncing.set(true);
        sync_error.set(None);
        spawn(async move {
            match backend.list_sync_now().await {
                Ok(r) => {
                    if r.errors.is_empty() {
                        snack.show(tf("lists.synced", &[("when", &t("time.now"))]));
                    } else {
                        snack.show(tf("lists.sync_partial", &[("errors", &r.errors.join("; "))]));
                    }
                    if r.conflicts > 0 {
                        conflicts_open.set(true);
                    }
                    lib.restart();
                    conflicts.restart();
                    let next = rev.peek().wrapping_add(1);
                        rev.set(next);
                }
                // No AniList / MAL account connected: explain where to connect one, nothing to fix here.
                Err(e) if e.kind == ErrorKind::Auth && e.detail == "not_connected" => snack.show(t("lists.not_connected")),
                Err(e) => sync_error.set(Some(e)),
            }
            syncing.set(false);
        });
    });

    let compact = window_class((ui.width)()) == WindowClass::Compact;
    let current = tab();
    let n_conflicts = match &*conflicts.read() {
        Some(Ok(c)) => c.len(),
        _ => 0,
    };
    let connected: Vec<String> = match &*accounts.read() {
        Some(Ok(list)) => list
            .iter()
            .filter(|a| a.connected)
            .map(|a| match &a.username {
                Some(u) => format!("{} · {u}", a.service.label()),
                None => a.service.label().to_string(),
            })
            .collect(),
        _ => Vec::new(),
    };
    let pill = if connected.is_empty() { t("lib.local") } else { connected.join(" / ") };
    let tablist_label = t("lib.title");

    // (tab id, label, selected, target) of the visible tab strip; `None` target = the Torrents shortcut.
    let strip: Vec<(String, String, bool, Option<LibTab>)> = if compact {
        let lists_tab = LibTab::List(last_status());
        vec![
            ("continue".into(), LibTab::Continue.label(), current == LibTab::Continue, Some(LibTab::Continue)),
            ("history".into(), LibTab::History.label(), current == LibTab::History, Some(LibTab::History)),
            ("lists".into(), t("lib.tab.lists"), matches!(current, LibTab::List(_)), Some(lists_tab)),
            ("favorites".into(), LibTab::Favorites.label(), current == LibTab::Favorites, Some(LibTab::Favorites)),
            ("torrents".into(), t("lib.tab.torrents"), false, None),
        ]
    } else {
        LibTab::expanded().into_iter().map(|x| (x.id().to_owned(), x.label(), current == x, Some(x))).collect()
    };
    let navigator = use_navigator();

    let content = match current {
        LibTab::Continue | LibTab::History => {
            let history = current == LibTab::History;
            match &*lib.read() {
                None => rsx! {
                    section { class: "tk-section", "aria-busy": "true",
                        if history {
                            SkeletonLines { widths: vec![90, 80, 70, 60] }
                        } else {
                            SkeletonGrid { count: 6 }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    ErrorState { error: e.clone(), retry: e.kind != ErrorKind::NotWired, on_retry: move |_| lib.restart() }
                },
                Some(Ok(vm)) => rsx! {
                    LocalTab { vm: vm.clone(), history }
                },
            }
        }
        LibTab::List(status) => rsx! {
            if compact {
                div { class: "tk-lib__seg",
                    Segmented {
                        options: ListStatusVm::ALL.iter().map(|s| (s.id().to_owned(), t(&status_key(*s)))).collect::<Vec<SegOption>>(),
                        selected: status.id().to_string(),
                        label: t("lib.tab.lists"),
                        small: true,
                        on_select: move |id: String| {
                            if let Some(s) = ListStatusVm::from_id(&id) {
                                last_status.set(s);
                                tab.set(LibTab::List(s));
                            }
                        },
                    }
                }
            }
            StatusTab { status, sort: sort(), query: query(), rev: rev() }
        },
        LibTab::Favorites => rsx! {
            FavoritesTab { sort: sort(), query: query(), rev: rev() }
        },
    };
    let list_tab = matches!(current, LibTab::List(_) | LibTab::Favorites);
    let sort_options: Vec<SegOption> = LibSort::ALL
        .iter()
        .filter(|s| current != LibTab::Favorites || **s != LibSort::Progress)
        .map(|s| (s.id().to_owned(), t(&format!("lib.sort.{}", s.id()))))
        .collect();

    rsx! {
        div { class: "tk-page tk-page--library", "data-compact": "{compact}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("lib.title")} }
                }
                div { class: "tk-lib__head-actions",
                    Pill { text: pill, tone: Tone::Primary }
                    SyncButton { syncing: syncing(), on_sync: sync }
                }
            }
            if let Some(e) = sync_error() {
                ErrorState { error: e, compact: true, title: Some(t("lists.sync_now")), on_retry: move |_| sync.call(()) }
            }
            if n_conflicts > 0 {
                div { class: "tk-lib__conflicts", "data-conflicts": "{n_conflicts}",
                    Banner { text: tn("lists.conflicts.count", n_conflicts), icon: "warning".to_string(), tone: Tone::Warn,
                        Button {
                            label: t("lists.conflicts.resolve"),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| conflicts_open.set(true),
                        }
                    }
                }
            }
            div { class: "tk-lib__tabs", role: "tablist", "aria-label": "{tablist_label}",
                for (id , text , selected , target) in strip {
                    button {
                        key: "{id}",
                        class: "tk-lib__tab",
                        r#type: "button",
                        role: "tab",
                        "data-status": "{id}",
                        "aria-selected": if selected { "true" } else { "false" },
                        tabindex: if selected { "0" } else { "-1" },
                        onclick: move |_| match target {
                            Some(next) => {
                                if let LibTab::List(s) = next {
                                    last_status.set(s);
                                }
                                tab.set(next);
                            }
                            None => {
                                navigator.push(Route::TorrentLibraryPage {});
                            }
                        },
                        "{text}"
                    }
                }
            }
            if list_tab {
                div { class: "tk-lib__tools",
                    TextField {
                        label: t("lib.filter"),
                        value: query(),
                        icon: "search".to_string(),
                        placeholder: t("lib.filter_hint"),
                        oninput: move |v: String| query.set(v),
                    }
                    Segmented {
                        options: sort_options,
                        selected: sort().id().to_string(),
                        label: t("lib.sort"),
                        small: true,
                        on_select: move |id: String| sort.set(LibSort::from_id(&id)),
                    }
                }
            }
            div { class: "tk-lib__panel", role: "tabpanel", "data-tab": "{current.id()}", {content} }
            if conflicts_open() {
                ConflictSheet {
                    on_close: move |_| conflicts_open.set(false),
                    on_changed: move |_| {
                        conflicts.restart();
                        let next = rev.peek().wrapping_add(1);
                        rev.set(next);
                    },
                }
            }
        }
    }
}

#[component]
fn SyncButton(syncing: bool, on_sync: EventHandler<()>) -> Element {
    let class = if syncing { "tk-lib__sync tk-lib__sync--busy" } else { "tk-lib__sync" }.to_string();
    rsx! {
        Button {
            label: t("lists.sync_now"),
            icon: "sync".to_string(),
            variant: ButtonVariant::Text,
            size: ButtonSize::Sm,
            disabled: syncing,
            class,
            onclick: move |_| on_sync.call(()),
        }
    }
}

/// Continue (poster grid) or History (episode rows) from `Backend::library()`.
#[component]
fn LocalTab(vm: LibraryVm, history: bool) -> Element {
    let navigator = use_navigator();
    let now = now_unix();
    if history {
        if vm.history.is_empty() {
            return rsx! {
                EmptyState { icon: "history".to_string(), title: t("lib.empty.title"), body: t("lib.empty.body") }
            };
        }
        return rsx! {
            div { class: "tk-list",
                for (i , h) in vm.history.clone().into_iter().enumerate() {
                    HistoryRow { key: "{h.media.id}-{h.episode}-{i}", item: h.clone(), now }
                }
            }
        };
    }
    if vm.continue_watching.is_empty() {
        return rsx! {
            EmptyState { icon: "library".to_string(), title: t("lib.empty.title"), body: t("lib.empty.body") }
        };
    }
    rsx! {
        div { class: "tk-grid",
            for c in vm.continue_watching.clone() {
                {
                    let (id, episode) = (c.media.id.clone(), c.episode);
                    let ep = episode.to_string();
                    let sub = format!(
                        "{} · {}",
                        tf("home.continue_episode", &[("n", &ep)]),
                        format_time(f64::from(c.progress_secs))
                    );
                    rsx! {
                        MediaCard {
                            key: "{c.media.id}",
                            media: c.media.clone(),
                            progress: c.fraction(),
                            badge: tf("media.ep_badge", &[("n", &ep)]),
                            sub,
                            onclick: move |_| {
                                navigator.push(Route::PlayerPage { id: id.clone(), episode });
                            },
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HistoryRow(item: HistoryItemVm, now: i64) -> Element {
    let navigator = use_navigator();
    let (id, episode) = (item.media.id.clone(), item.episode);
    let ep = episode.to_string();
    let sub = format!(
        "{} · {}",
        tf("details.episode", &[("n", &ep)]),
        tf("lib.watched_on", &[("when", &rel_time(now, item.watched_unix))])
    );
    let row = EpisodeVm { number: episode, title: None, image: None, air_date: None, is_filler: false, watched: 0.0 };
    rsx! {
        EpisodeRow {
            episode: row,
            color: item.media.color.clone(),
            title: item.media.title.clone(),
            sub,
            pills: vec![(t("details.watched"), Tone::Ok)],
            trailing_icon: "chevron-right".to_string(),
            onclick: move |_| {
                navigator.push(Route::PlayerPage { id: id.clone(), episode });
            },
        }
    }
}

/// One status of the local list mirror.
#[component]
fn StatusTab(status: ListStatusVm, sort: LibSort, query: String, rev: u32) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut entries = use_resource(use_reactive((&status, &rev), {
        let backend = services.backend.clone();
        move |(status, _rev)| {
            let backend = backend.clone();
            async move { backend.list_entries(Some(status)).await }
        }
    }));
    let busy = use_signal(|| None::<String>);
    let set_progress = use_callback({
        let backend = services.backend.clone();
        move |(media_id, progress): (String, u32)| {
            let (mut busy, mut entries) = (busy, entries);
            if busy.peek().is_some() {
                return;
            }
            busy.set(Some(media_id.clone()));
            let backend = backend.clone();
            spawn(async move {
                let update = ListEntryUpdateVm { progress: Some(progress), ..ListEntryUpdateVm::default() };
                if let Err(e) = backend.set_list_entry(&media_id, &update).await {
                    snack.show(format!("{} {}", t("lists.error_save"), t(e.kind.key())));
                }
                entries.restart();
                busy.set(None);
            });
        }
    });

    let body = match &*entries.read() {
        None => rsx! {
            div { "aria-busy": "true", SkeletonLines { widths: vec![90, 80, 70, 60] } }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), retry: e.kind != ErrorKind::NotWired, on_retry: move |_| entries.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState {
                icon: "bookmark".to_string(),
                title: tf("lib.list_empty.title", &[("status", &t(&status_key(status)))]),
                body: t("lib.list_empty.body"),
            }
        },
        Some(Ok(list)) => {
            let shown = sort_entries(list.clone(), sort, &query);
            if shown.is_empty() {
                rsx! {
                    EmptyState { icon: "filter".to_string(), title: t("lib.no_match") }
                }
            } else {
                let n = shown.len();
                rsx! {
                    p { class: "tk-lib__count tk-muted", {tn("lib.count", n)} }
                    div { class: "tk-list tk-lentries",
                        for e in shown {
                            EntryRow {
                                key: "{e.media.id}",
                                busy: busy().as_deref() == Some(e.media.id.as_str()),
                                entry: e.clone(),
                                on_progress: move |p: (String, u32)| set_progress.call(p),
                            }
                        }
                    }
                }
            }
        }
    };
    rsx! {
        section { class: "tk-section", "data-list-tab": "{status.id()}", {body} }
    }
}

#[component]
fn EntryRow(entry: ListEntryVm, busy: bool, on_progress: EventHandler<(String, u32)>) -> Element {
    let navigator = use_navigator();
    let id = entry.media.id.clone();
    let (progress, total) = (entry.progress, entry.episodes_total);
    let score = match entry.score {
        Some(s) if s > 0 => score_text(s, entry.service),
        _ => t("lists.score_none"),
    };
    let (dec_id, inc_id) = (id.clone(), id.clone());
    let at_end = total.is_some_and(|t| t > 0 && progress >= t);
    rsx! {
        div {
            class: "tk-lentry",
            "data-media-id": "{id}",
            "data-list-status": "{entry.status.id()}",
            "data-list-progress": "{progress}",
            "data-pending": "{entry.pending}",
            button {
                class: "tk-lentry__open",
                r#type: "button",
                onclick: move |_| {
                    navigator.push(Route::DetailsPage { id: id.clone() });
                },
                div { class: "tk-lentry__cover",
                    MediaCover { src: entry.media.cover.clone(), color: entry.media.color.clone(), title: entry.media.title.clone() }
                }
                div { class: "tk-lentry__text",
                    div { class: "tk-lentry__title", "{entry.media.title}" }
                    div { class: "tk-lentry__sub tk-muted",
                        {t(&status_key(entry.status))}
                        " · "
                        "{score}"
                        if entry.repeats > 0 {
                            " · "
                            {tf("lists.repeats", &[("n", &entry.repeats.to_string())])}
                        }
                    }
                }
            }
            div { class: "tk-lentry__progress",
                IconButton {
                    icon: "minus".to_string(),
                    label: t("lists.progress_dec"),
                    size: IconButtonSize::Sm,
                    disabled: busy || progress == 0,
                    onclick: move |_| on_progress.call((dec_id.clone(), step_progress(progress, -1, total))),
                }
                span { class: "tk-lentry__value", {progress_text(progress, total)} }
                IconButton {
                    icon: "plus".to_string(),
                    label: t("lists.progress_inc"),
                    size: IconButtonSize::Sm,
                    disabled: busy || at_end,
                    onclick: move |_| on_progress.call((inc_id.clone(), step_progress(progress, 1, total))),
                }
            }
            if entry.pending {
                span { class: "tk-lentry__pending", title: t("lists.pending"), "aria-label": t("lists.pending"),
                    Icon { name: "sync".to_string(), size: 16 }
                }
            }
        }
    }
}

/// Favourite titles and characters (`character:<id>` rows open the character page).
#[component]
fn FavoritesTab(sort: LibSort, query: String, rev: u32) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut favs = use_resource(use_reactive((&rev,), {
        let backend = services.backend.clone();
        move |(_rev,)| {
            let backend = backend.clone();
            async move { backend.favorites().await }
        }
    }));
    let body = match &*favs.read() {
        None => rsx! { div { "aria-busy": "true", SkeletonGrid { count: 6 } } },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), retry: e.kind != ErrorKind::NotWired, on_retry: move |_| favs.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "star".to_string(), title: t("lib.fav_empty.title"), body: t("lib.fav_empty.body") }
        },
        Some(Ok(list)) => {
            let shown = sort_media(list.clone(), sort, &query);
            if shown.is_empty() {
                rsx! { EmptyState { icon: "filter".to_string(), title: t("lib.no_match") } }
            } else {
                rsx! {
                    div { class: "tk-grid", "data-favorites": "{shown.len()}",
                        for m in shown {
                            {
                                let target = match m.id.strip_prefix(CHARACTER_PREFIX) {
                                    Some(c) => Route::CharacterPage { id: c.to_owned() },
                                    None => Route::DetailsPage { id: m.id.clone() },
                                };
                                let character = m.id.starts_with(CHARACTER_PREFIX);
                                let badge = if character { t("lib.fav_character") } else { String::new() };
                                rsx! {
                                    MediaCard {
                                        key: "{m.id}",
                                        media: m.clone(),
                                        badge,
                                        hide_score: true,
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
    };
    rsx! {
        section { class: "tk-section", "data-list-tab": "favorites", {body} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ListServiceVm;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(rel_time_parts(30), ("time.now", None));
        assert_eq!(rel_time_parts(5 * 60), ("time.min_ago", Some(5)));
        assert_eq!(rel_time_parts(3 * 3600 + 10 * 60), ("time.h_ago", Some(3)));
        assert_eq!(rel_time_parts(30 * 3600), ("time.yesterday", None));
        assert_eq!(rel_time_parts(5 * 86_400), ("time.d_ago", Some(5)));
        assert_eq!(rel_time_parts(-5 * 60), ("time.min_ago", Some(5)));
    }

    #[test]
    fn nine_tabs_with_unique_ids() {
        let tabs = LibTab::expanded();
        assert_eq!(tabs.len(), 9);
        let ids: std::collections::BTreeSet<_> = tabs.iter().map(|t| t.id()).collect();
        assert_eq!(ids.len(), 9);
        assert_eq!(tabs[0], LibTab::Continue);
        assert_eq!(tabs[8], LibTab::Favorites);
    }

    fn entry(title: &str, progress: u32, score: Option<u8>, updated: i64) -> ListEntryVm {
        ListEntryVm {
            media: MediaVm {
                id: title.to_owned(),
                title: title.to_owned(),
                cover: None,
                color: None,
                subtitle: String::new(),
                score: None,
            },
            service: ListServiceVm::AniList,
            status: ListStatusVm::Watching,
            progress,
            episodes_total: None,
            score,
            started: None,
            finished: None,
            repeats: 0,
            notes: None,
            updated_unix: updated,
            remote_id: None,
            pending: false,
        }
    }

    fn titles(v: &[ListEntryVm]) -> Vec<&str> {
        v.iter().map(|e| e.media.title.as_str()).collect()
    }

    #[test]
    fn sorting_and_filtering() {
        let list = vec![entry("Bleach", 3, Some(70), 10), entry("akira", 1, None, 30), entry("Cowboy", 9, Some(90), 20)];
        assert_eq!(titles(&sort_entries(list.clone(), LibSort::Updated, "")), ["akira", "Cowboy", "Bleach"]);
        assert_eq!(titles(&sort_entries(list.clone(), LibSort::Title, "")), ["akira", "Bleach", "Cowboy"]);
        assert_eq!(titles(&sort_entries(list.clone(), LibSort::Score, "")), ["Cowboy", "Bleach", "akira"]);
        assert_eq!(titles(&sort_entries(list.clone(), LibSort::Progress, "")), ["Cowboy", "Bleach", "akira"]);
        assert_eq!(titles(&sort_entries(list, LibSort::Title, " BLE ")), ["Bleach"]);
        assert_eq!(LibSort::from_id("nope"), LibSort::Updated);
    }

    #[test]
    fn favourites_sort() {
        let m = |t: &str| MediaVm { id: t.into(), title: t.into(), cover: None, color: None, subtitle: String::new(), score: None };
        let v = sort_media(vec![m("b"), m("A"), m("c")], LibSort::Title, "");
        assert_eq!(v.iter().map(|m| m.title.as_str()).collect::<Vec<_>>(), ["A", "b", "c"]);
        assert_eq!(sort_media(vec![m("b"), m("A")], LibSort::Updated, "a").len(), 1);
    }
}
