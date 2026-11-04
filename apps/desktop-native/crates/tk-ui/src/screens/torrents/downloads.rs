//! Torrent client > Downloads: background downloads. Until the user accepts the warning (CPU, disk and network use,
//! uploading to other peers while the app is open) the page shows only that warning with an "I understand" button
//! (`background_downloads_ack`). Afterwards: the background-downloads switch (`save_torrent_settings`) and the list
//! of torrents still downloading for later (kept, not complete, not streaming right now) with speed / peers / ETA
//! from `torrent_stats`, pause / resume / open in the client / remove. Polled every 2 s while mounted.
//! Owner: wave 3 D.

use std::time::Duration;

use dioxus::prelude::*;

use super::library::{POLL_SECS, display_title, format_bytes, format_duration, format_percent, format_speed, state_tone};
use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, TorrentLibraryItemVm, TorrentSettingsVm, TorrentStateVm, TorrentStatsVm, UiResult};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize,
    LoadingIndicator, Pill, SkeletonLines, Switch, Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::UiError;

/// One background download: the library entry plus live stats (`None` when the engine could not tell).
#[derive(Debug, Clone, PartialEq)]
struct DownloadRow {
    item: TorrentLibraryItemVm,
    stats: Option<TorrentStatsVm>,
}

/// A torrent kept for later that still has data to fetch. Streaming sessions are filtered out with the stats.
fn is_background_candidate(item: &TorrentLibraryItemVm) -> bool {
    item.persist && item.progress < 1.0 && item.state != TorrentStateVm::Seeding
}

/// Rows of the list: candidates in library order (newest first) minus what is being streamed right now.
async fn load_rows(backend: &BackendRef) -> UiResult<Vec<DownloadRow>> {
    let items = backend.torrent_library().await?;
    let mut rows = Vec::new();
    for item in items.into_iter().filter(is_background_candidate) {
        let stats = backend.torrent_stats(&item.id).await.ok();
        if stats.as_ref().is_some_and(|s| s.streaming) {
            continue;
        }
        rows.push(DownloadRow { item, stats });
    }
    Ok(rows)
}

#[component]
pub fn TorrentDownloadsPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let mut settings = use_signal(|| None::<UiResult<TorrentSettingsVm>>);
    let mut rows = use_signal(|| None::<UiResult<Vec<DownloadRow>>>);
    let mut busy = use_signal(|| false);
    let mut action_error = use_signal(|| None::<UiError>);
    let mut confirm = use_signal(|| None::<String>);
    let mut delete_files = use_signal(|| false);

    let reload_settings = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                settings.set(Some(backend.torrent_settings().await));
            });
        }
    });

    let refresh = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                rows.set(Some(load_rows(&backend).await));
            });
        }
    });

    // Settings once, then the list every POLL_SECS while the warning is accepted.
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                settings.set(Some(backend.torrent_settings().await));
                loop {
                    let acked = matches!(&*settings.peek(), Some(Ok(s)) if s.background_ack);
                    if acked {
                        let next = load_rows(&backend).await;
                        if (*rows.peek()).as_ref() != Some(&next) {
                            rows.set(Some(next));
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
                }
            }
        }
    });

    let accept = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.background_downloads_ack().await {
                    Ok(()) => {
                        settings.set(Some(backend.torrent_settings().await));
                        rows.set(Some(load_rows(&backend).await));
                    }
                    Err(e) => action_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let set_enabled = use_callback({
        let backend = backend.clone();
        move |on: bool| {
            let Some(Ok(current)) = settings() else { return };
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let next = TorrentSettingsVm { background_downloads: on, ..current };
            let backend = backend.clone();
            spawn(async move {
                match backend.save_torrent_settings(&next).await {
                    Ok(()) => {
                        snack.show(t(if on { "torrent.downloads.enabled" } else { "torrent.downloads.disabled" }));
                        settings.set(Some(backend.torrent_settings().await));
                    }
                    Err(e) => action_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let pause_resume = use_callback({
        let backend = backend.clone();
        move |(id, pause): (String, bool)| {
            let backend = backend.clone();
            spawn(async move {
                let r = if pause { backend.torrent_pause(&id).await } else { backend.torrent_resume(&id).await };
                if let Err(e) = r {
                    action_error.set(Some(e));
                }
                refresh.call(());
            });
        }
    });

    let open = use_callback(move |id: String| {
        let mut current = ui.torrent;
        current.set(Some(id));
        navigator.push(Route::TorrentOverviewPage {});
    });

    let remove = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Some(id) = confirm() else { return };
            if busy() {
                return;
            }
            let with_files = delete_files();
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.torrent_remove(&id, with_files).await {
                    Ok(()) => snack.show(t("torrent.downloads.removed")),
                    Err(e) => action_error.set(Some(e)),
                }
                confirm.set(None);
                busy.set(false);
                refresh.call(());
            });
        }
    });

    let content = match &*settings.read() {
        None => rsx! {
            div { class: "tk-tdl__loading", LoadingIndicator {} }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: Some(t("torrent.downloads.settings_error")), on_retry: move |_| reload_settings.call(()) }
        },
        Some(Ok(s)) if !s.background_ack => rsx! {
            WarningCard { busy: busy(), on_accept: move |_| accept.call(()) }
        },
        Some(Ok(s)) => {
            let enabled = s.background_downloads;
            let list = match &*rows.read() {
                None => rsx! {
                    div { "aria-busy": "true", SkeletonLines { widths: vec![100, 90, 80] } }
                },
                Some(Err(e)) => rsx! {
                    ErrorState { error: e.clone(), title: Some(t("torrent.downloads.error")), on_retry: move |_| refresh.call(()) }
                },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    EmptyState {
                        icon: "download".to_string(),
                        title: t("torrent.downloads.empty.title"),
                        body: t(if enabled { "torrent.downloads.empty.body" } else { "torrent.downloads.empty.body_off" }),
                    }
                },
                Some(Ok(list)) => rsx! {
                    ul { class: "tk-tdl__list", role: "list",
                        for row in list.iter().cloned() {
                            DownloadCard {
                                key: "{row.item.id}",
                                row: row.clone(),
                                busy: busy(),
                                on_pause: move |change: (String, bool)| pause_resume.call(change),
                                on_open: move |id: String| open.call(id),
                                on_remove: move |id: String| {
                                    delete_files.set(false);
                                    confirm.set(Some(id));
                                },
                            }
                        }
                    }
                },
            };
            rsx! {
                div { class: "tk-tdl__toggle",
                    div { class: "tk-tdl__toggle-text",
                        span { class: "tk-tdl__toggle-label", {t("torrent.downloads.switch")} }
                        span { class: "tk-tdl__toggle-hint", {t("torrent.downloads.switch_hint")} }
                    }
                    Switch {
                        checked: enabled,
                        label: t("torrent.downloads.switch"),
                        disabled: busy(),
                        on_toggle: move |v: bool| set_enabled.call(v),
                    }
                }
                if !enabled {
                    Banner { text: t("torrent.downloads.off_banner"), icon: "info".to_string(), tone: Tone::Neutral }
                }
                {list}
            }
        }
    };

    let dialog = confirm().map(|_| {
        rsx! {
            ConfirmDialog {
                title: t("torrent.downloads.remove_title"),
                body: t("torrent.library.delete_body"),
                confirm_label: t("common.remove"),
                icon: "trash".to_string(),
                confirm_disabled: busy(),
                on_confirm: move |_| remove.call(()),
                on_cancel: move |_| {
                    if !busy() {
                        confirm.set(None);
                    }
                },
                div { class: "tk-tlib__files-opt",
                    Switch {
                        checked: delete_files(),
                        label: t("torrent.library.delete_files"),
                        on_toggle: move |v: bool| delete_files.set(v),
                    }
                    div { class: "tk-tlib__files-text",
                        span { class: "tk-tlib__files-label", {t("torrent.library.delete_files")} }
                        span { class: "tk-tlib__files-hint", {t("torrent.library.delete_files_hint")} }
                    }
                }
            }
        }
    });

    rsx! {
        section { class: "tk-tdl", "aria-label": t("torrent.section.downloads"),
            div { class: "tk-tlib__head",
                h2 { class: "tk-tlib__title", {t("torrent.section.downloads")} }
                p { class: "tk-tlib__intro", {t("torrent.downloads.intro")} }
            }
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false, title: Some(t("torrent.library.action_failed")),
                    Button { label: t("common.close"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| action_error.set(None) }
                }
            }
            {content}
        }
        {dialog}
    }
}

/// The "I understand" warning shown until the user accepts it.
#[component]
fn WarningCard(busy: bool, on_accept: EventHandler<()>) -> Element {
    let points = [
        ("gpu", "torrent.downloads.warn.cpu"),
        ("storage", "torrent.downloads.warn.disk"),
        ("download", "torrent.downloads.warn.network"),
        ("upload", "torrent.downloads.warn.upload"),
    ];
    rsx! {
        div { class: "tk-tdl__warn", role: "region", "aria-labelledby": "tk-tdl-warn-title",
            div { class: "tk-tdl__warn-icon", Icon { name: "warning".to_string(), size: 28 } }
            h3 { class: "tk-tdl__warn-title", id: "tk-tdl-warn-title", {t("torrent.downloads.warn.title")} }
            p { class: "tk-tdl__warn-body", {t("torrent.downloads.warn.body")} }
            ul { class: "tk-tdl__warn-list",
                for (icon , key) in points {
                    li { key: "{key}", class: "tk-tdl__warn-item",
                        Icon { name: icon.to_string(), size: 20 }
                        span { {t(key)} }
                    }
                }
            }
            p { class: "tk-tdl__warn-note", {t("torrent.downloads.warn.note")} }
            div { class: "tk-tdl__warn-actions",
                Button {
                    label: t("torrent.downloads.warn.accept"),
                    icon: "check".to_string(),
                    disabled: busy,
                    onclick: move |_| on_accept.call(()),
                }
            }
        }
    }
}

#[component]
fn DownloadCard(
    row: DownloadRow,
    busy: bool,
    /// `(id, pause)`: `true` pauses, `false` resumes.
    on_pause: EventHandler<(String, bool)>,
    on_open: EventHandler<String>,
    on_remove: EventHandler<String>,
) -> Element {
    let item = &row.item;
    let title = display_title(item);
    let episode = item.episode.map(|e| tf("torrent.downloads.episode", &[("n", &e.to_string())]));
    let state = row.stats.as_ref().map(|s| s.state).unwrap_or(item.state);
    let progress = row.stats.as_ref().map(|s| s.progress).unwrap_or(item.progress);
    let pct = format_percent(progress);
    let paused = state == TorrentStateVm::Paused;
    let state_text = t(&format!("torrent.state.{}", state.id()));
    let progress_label = tf("torrent.library.progress", &[("pct", &pct)]);
    let sizes = match &row.stats {
        Some(s) => tf(
            "torrent.downloads.of",
            &[("done", &format_bytes(s.downloaded_bytes.min(s.total_bytes))), ("total", &format_bytes(s.total_bytes))],
        ),
        None => format_bytes(item.size_bytes),
    };
    let speeds = row.stats.as_ref().filter(|_| !paused).map(|s| {
        (
            tf("torrent.downloads.down", &[("speed", &format_speed(s.down_bps))]),
            tf("torrent.downloads.up", &[("speed", &format_speed(s.up_bps))]),
            tf("torrent.downloads.peers", &[("n", &s.peers.to_string())]),
            s.eta_secs.map(|e| tf("torrent.downloads.eta", &[("time", &format_duration(e))])),
        )
    });
    let error = row.stats.as_ref().and_then(|s| s.error.clone()).filter(|_| state == TorrentStateVm::Error);
    let (id1, id2, id3) = (item.id.clone(), item.id.clone(), item.id.clone());
    rsx! {
        li { class: "tk-tdl__item", "data-state": state.id(),
            div { class: "tk-tdl__item-head",
                div { class: "tk-tdl__item-titles",
                    span { class: "tk-tdl__item-title", title: "{title}", "{title}" }
                    if let Some(ep) = episode {
                        span { class: "tk-tdl__item-ep", "{ep}" }
                    }
                    span { class: "tk-tdl__item-name", title: "{item.name}", "{item.name}" }
                }
                Pill { text: state_text, tone: state_tone(state) }
            }
            div { class: "tk-tdl__item-progress",
                WavyProgress { fraction: progress, label: progress_label, active: state == TorrentStateVm::Downloading }
                span { class: "tk-tdl__pct", "{pct}" }
            }
            div { class: "tk-tdl__item-stats",
                span { class: "tk-tdl__stat", "{sizes}" }
                if let Some((down, up, peers, eta)) = speeds {
                    span { class: "tk-tdl__stat", Icon { name: "download".to_string(), size: 16 } "{down}" }
                    span { class: "tk-tdl__stat", Icon { name: "upload".to_string(), size: 16 } "{up}" }
                    span { class: "tk-tdl__stat", "{peers}" }
                    if let Some(eta) = eta {
                        span { class: "tk-tdl__stat", Icon { name: "clock".to_string(), size: 16 } "{eta}" }
                    }
                }
            }
            if let Some(message) = error {
                p { class: "tk-tdl__item-error", role: "alert", "{message}" }
            }
            div { class: "tk-tdl__item-actions",
                if paused {
                    Button {
                        label: t("torrent.downloads.resume"),
                        icon: "play".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: busy,
                        onclick: move |_| on_pause.call((id1.clone(), false)),
                    }
                } else {
                    Button {
                        label: t("torrent.downloads.pause"),
                        icon: "pause".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: busy,
                        onclick: move |_| on_pause.call((id1.clone(), true)),
                    }
                }
                IconButton {
                    icon: "external".to_string(),
                    label: t("torrent.library.open"),
                    size: IconButtonSize::Sm,
                    onclick: move |_| on_open.call(id2.clone()),
                }
                IconButton {
                    icon: "trash".to_string(),
                    label: t("torrent.downloads.remove"),
                    size: IconButtonSize::Sm,
                    disabled: busy,
                    onclick: move |_| on_remove.call(id3.clone()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_kept_unfinished_torrents_are_background_downloads() {
        let base = TorrentLibraryItemVm {
            id: "a".into(),
            name: "x".into(),
            media_id: None,
            series: None,
            episode: None,
            poster: None,
            files: 1,
            size_bytes: 1,
            progress: 0.3,
            state: TorrentStateVm::Downloading,
            added_unix: 0,
            persist: true,
            path: String::new(),
        };
        assert!(is_background_candidate(&base));
        assert!(is_background_candidate(&TorrentLibraryItemVm { state: TorrentStateVm::Paused, ..base.clone() }));
        assert!(!is_background_candidate(&TorrentLibraryItemVm { persist: false, ..base.clone() }));
        assert!(!is_background_candidate(&TorrentLibraryItemVm { progress: 1.0, ..base.clone() }));
        assert!(!is_background_candidate(&TorrentLibraryItemVm { state: TorrentStateVm::Seeding, ..base }));
    }
}
