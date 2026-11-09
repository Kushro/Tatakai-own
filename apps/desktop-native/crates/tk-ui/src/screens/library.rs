//! Library (M0: local on this PC): Continue watching grid + watch history from `Backend::library()`
//! (docs/redesign/07 section 4, prototype `screens.js::library`). "Sync lists" calls `Backend::list_sync_now`
//! (wave A, worker g): success and "no account connected" (`ErrorKind::Auth`) are confirmations in the snackbar;
//! any other failure stays inline. History itself never syncs from here: it is local and paired phones pull it
//! over Tatakai Link (plan 14 section 0.1 #9).

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{EpisodeVm, HistoryItemVm, LibraryVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, EpisodeRow, ErrorState, MediaCard, Pill, SkeletonGrid,
    SkeletonLines, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::{ErrorKind, UiError, format_time};
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

#[component]
pub fn LibraryPage() -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut lib = use_resource(move || {
        let backend = backend.clone();
        async move { backend.library().await }
    });
    let syncing = use_signal(|| false);
    let sync_error = use_signal(|| None::<UiError>);

    let sync_backend = services.backend.clone();
    let sync = use_callback(move |_: ()| {
        if *syncing.peek() {
            return;
        }
        let backend = sync_backend.clone();
        let (mut syncing, mut sync_error, mut lib) = (syncing, sync_error, lib);
        syncing.set(true);
        sync_error.set(None);
        spawn(async move {
            match backend.list_sync_now().await {
                Ok(_) => {
                    snack.show(tf("lists.synced", &[("when", &t("time.now"))]));
                    lib.restart();
                }
                // No AniList / MAL account connected: explain where to connect one, nothing to fix here.
                Err(e) if e.kind == ErrorKind::Auth => snack.show(t("lists.not_connected")),
                Err(e) => sync_error.set(Some(e)),
            }
            syncing.set(false);
        });
    });

    let body = match &*lib.read() {
        None => rsx! {
            section { class: "tk-section", "aria-busy": "true",
                SkeletonGrid { count: 6 }
            }
            section { class: "tk-section", "aria-busy": "true",
                SkeletonLines { widths: vec![90, 80, 70, 60] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), retry: e.kind != ErrorKind::NotWired, on_retry: move |_| lib.restart() }
        },
        Some(Ok(vm)) if vm.continue_watching.is_empty() && vm.history.is_empty() => rsx! {
            EmptyState { icon: "library".to_string(), title: t("lib.empty.title"), body: t("lib.empty.body"),
                SyncButton { syncing: syncing(), on_sync: sync }
            }
        },
        Some(Ok(vm)) => rsx! {
            LibraryContent { vm: vm.clone(), syncing: syncing(), on_sync: sync }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--library",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("lib.title")} }
                    p { class: "tk-page__intro", {t("lib.m1")} }
                }
                Pill { text: t("lib.local"), tone: Tone::Primary }
            }
            if let Some(e) = sync_error() {
                ErrorState { error: e, compact: true, title: Some(t("lists.sync_now")), on_retry: move |_| sync.call(()) }
            }
            {body}
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

#[component]
fn LibraryContent(vm: LibraryVm, syncing: bool, on_sync: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    let now = now_unix();
    let n_cont = vm.continue_watching.len();
    let n_hist = vm.history.len();
    rsx! {
        if n_cont > 0 {
            section { class: "tk-section",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title",
                        {t("home.continue")}
                        small { class: "tk-section__count", "{n_cont}" }
                    }
                }
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
        section { class: "tk-section",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title",
                    {t("lib.history")}
                    small { class: "tk-section__count", "{n_hist}" }
                }
                SyncButton { syncing, on_sync }
            }
            if n_hist == 0 {
                p { class: "tk-section__empty tk-muted", {t("lib.empty.body")} }
            } else {
                div { class: "tk-list",
                    for (i , h) in vm.history.clone().into_iter().enumerate() {
                        HistoryRow { key: "{h.media.id}-{h.episode}-{i}", item: h.clone(), now }
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

#[cfg(test)]
mod tests {
    use super::rel_time_parts;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(rel_time_parts(30), ("time.now", None));
        assert_eq!(rel_time_parts(5 * 60), ("time.min_ago", Some(5)));
        assert_eq!(rel_time_parts(3 * 3600 + 10 * 60), ("time.h_ago", Some(3)));
        assert_eq!(rel_time_parts(30 * 3600), ("time.yesterday", None));
        assert_eq!(rel_time_parts(5 * 86_400), ("time.d_ago", Some(5)));
        assert_eq!(rel_time_parts(-5 * 60), ("time.min_ago", Some(5)));
    }
}
