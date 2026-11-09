//! Torrents > History (wave A, worker **i**; plan 14 section 0.1 #11, checklist `torrent.session-history`): every
//! torrent the engine ran, newest first (`Backend::torrent_session_history`), with series / episode, size moved
//! (downloaded / uploaded), started / ended, completed; "Open in client" for rows still in the engine, "Add again"
//! for closed ones (`torrent_add_manual` with the stored info-hash), and "Clear history" (`torrent_clear_session_history`,
//! `ConfirmDialog`). Route `/torrents/history`. Rows expose `data-session-id` and `data-session-open="true|false"`.

use dioxus::prelude::*;

use super::layout::common::format_bytes;
use crate::app::{Route, Services, Ui};
use crate::backend::{TorrentSessionVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize, Pill, SkeletonLines,
    Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{UiError, format_unix_utc};

/// Rows asked from the backend.
const HISTORY_LIMIT: u32 = 300;

/// `YYYY-MM-DD HH:MM` (UTC) of a Unix time.
fn short_time(unix: i64) -> String {
    format_unix_utc(unix).chars().take(16).collect()
}

/// `"Show · E03"`, `"Show"`, or the torrent name for rows without a title.
fn session_title(s: &TorrentSessionVm) -> String {
    let series = s.series.clone().filter(|x| !x.trim().is_empty());
    match (series, s.episode) {
        (Some(series), Some(ep)) => tf("torrent.history.series_episode", &[("series", &series), ("episode", &ep.to_string())]),
        (Some(series), None) => series,
        (None, _) => s.name.clone(),
    }
}

#[component]
pub fn TorrentHistoryPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let mut rows = use_signal(|| None::<UiResult<Vec<TorrentSessionVm>>>);
    let mut confirm = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut action_error = use_signal(|| None::<UiError>);

    let reload = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                rows.set(Some(backend.torrent_session_history(HISTORY_LIMIT).await));
            });
        }
    });
    use_hook(|| reload.call(()));

    let clear = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.torrent_clear_session_history().await {
                    Ok(n) => snack.show(tn("torrent.history.cleared", n as usize)),
                    Err(e) => action_error.set(Some(e)),
                }
                confirm.set(false);
                busy.set(false);
                reload.call(());
            });
        }
    });

    let open_in_client = use_callback(move |torrent_id: String| {
        let mut current = ui.torrent;
        current.set(Some(torrent_id));
        navigator.push(Route::TorrentOverviewPage {});
    });

    let add_again = use_callback({
        let backend = backend.clone();
        move |s: TorrentSessionVm| {
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.torrent_add_manual(&s.info_hash, s.media_id.as_deref(), &[]).await {
                    Ok(id) => {
                        snack.show(t("torrent.history.added_again"));
                        let mut current = ui.torrent;
                        current.set(Some(id));
                        navigator.push(Route::TorrentOverviewPage {});
                    }
                    Err(e) => action_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let has_closed = matches!(&*rows.read(), Some(Ok(list)) if list.iter().any(|s| s.ended_unix.is_some()));
    let body = match &*rows.read() {
        None => rsx! {
            div { "aria-busy": "true", SkeletonLines { widths: vec![100, 90, 80, 70] } }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: Some(t("torrent.history.error")), on_retry: move |_| reload.call(()) }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "history".to_string(), title: t("torrent.history.empty_title"), body: t("torrent.history.empty_body") }
        },
        Some(Ok(list)) => {
            let count = tn("torrent.history.count", list.len());
            rsx! {
                p { class: "tk-ttable__summary tk-num", "{count}" }
                ul { class: "tk-thistory__list", "data-history": "true",
                    for s in list.clone() {
                        HistoryRow {
                            key: "{s.id}",
                            session: s.clone(),
                            busy: busy(),
                            on_open: move |id: String| open_in_client.call(id),
                            on_add: move |s: TorrentSessionVm| add_again.call(s),
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "tk-thistory", "aria-label": t("torrent.section.history"),
            div { class: "tk-tlib__head",
                h2 { class: "tk-tlib__title", {t("torrent.section.history")} }
                p { class: "tk-tlib__intro", {t("torrent.history.intro")} }
            }
            div { class: "tk-tlib__bar", role: "toolbar",
                Button {
                    label: t("torrent.history.refresh"),
                    icon: "refresh".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: move |_| reload.call(()),
                }
                if has_closed {
                    Button {
                        label: t("torrent.history.clear"),
                        icon: "trash".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: busy(),
                        onclick: move |_| confirm.set(true),
                    }
                }
            }
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false, title: Some(t("torrent.history.action_failed")),
                    Button { label: t("common.close"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| action_error.set(None) }
                }
            }
            {body}
        }
        if confirm() {
            ConfirmDialog {
                title: t("torrent.history.clear_title"),
                body: t("torrent.history.clear_body"),
                confirm_label: t("torrent.history.clear"),
                icon: "trash".to_string(),
                confirm_disabled: busy(),
                on_confirm: move |_| clear.call(()),
                on_cancel: move |_| confirm.set(false),
            }
        }
    }
}

#[component]
fn HistoryRow(
    session: TorrentSessionVm,
    busy: bool,
    on_open: EventHandler<String>,
    on_add: EventHandler<TorrentSessionVm>,
) -> Element {
    let s = session;
    let open = s.ended_unix.is_none();
    let title = session_title(&s);
    let show_name = title != s.name;
    let started = short_time(s.started_unix);
    let when = match s.ended_unix {
        Some(end) => tf("torrent.history.when_closed", &[("start", &started), ("end", &short_time(end))]),
        None => tf("torrent.history.when_open", &[("start", &started)]),
    };
    let moved = tf(
        "torrent.history.moved",
        &[("down", &format_bytes(s.downloaded_bytes)), ("up", &format_bytes(s.uploaded_bytes))],
    );
    let (state_text, tone) = if open {
        (t("torrent.history.state_open"), Tone::Primary)
    } else if s.completed {
        (t("torrent.history.state_completed"), Tone::Ok)
    } else {
        (t("torrent.history.state_closed"), Tone::Neutral)
    };
    let icon = if open { "play" } else { "history" };
    let torrent_id = s.torrent_id.clone();
    let again = s.clone();
    rsx! {
        li {
            class: "tk-thistory__row",
            "data-session-id": "{s.id}",
            "data-session-open": "{open}",
            "data-completed": "{s.completed}",
            span { class: "tk-thistory__icon", "aria-hidden": "true",
                Icon { name: icon.to_string(), size: 20 }
            }
            div { class: "tk-thistory__text",
                span { class: "tk-thistory__title", title: "{s.name}", "{title}" }
                if show_name {
                    span { class: "tk-thistory__name tk-selectable", title: "{s.name}", "{s.name}" }
                }
                span { class: "tk-thistory__meta tk-num", "{when} · {moved}" }
            }
            Pill { text: state_text, tone: tone }
            div { class: "tk-thistory__actions",
                if open {
                    IconButton {
                        icon: "external".to_string(),
                        label: t("torrent.history.open"),
                        size: IconButtonSize::Sm,
                        onclick: move |_| on_open.call(torrent_id.clone()),
                    }
                } else {
                    IconButton {
                        icon: "replay".to_string(),
                        label: t("torrent.history.add_again"),
                        size: IconButtonSize::Sm,
                        disabled: busy,
                        onclick: move |_| on_add.call(again.clone()),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(series: Option<&str>, episode: Option<u32>) -> TorrentSessionVm {
        TorrentSessionVm {
            id: 1,
            torrent_id: "a".repeat(40),
            info_hash: "a".repeat(40),
            name: "[Group] Show - 03.mkv".into(),
            media_id: None,
            series: series.map(str::to_owned),
            episode,
            started_unix: 1_700_000_000,
            ended_unix: None,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            completed: false,
        }
    }

    #[test]
    fn titles_and_times() {
        assert_eq!(session_title(&session(None, Some(3))), "[Group] Show - 03.mkv");
        assert_eq!(session_title(&session(Some("Show"), None)), "Show");
        assert_eq!(session_title(&session(Some("  "), None)), "[Group] Show - 03.mkv");
        assert_eq!(short_time(1_700_000_000), "2023-11-14 22:13");
    }
}
