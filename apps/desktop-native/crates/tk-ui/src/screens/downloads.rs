//! Downloads page `/downloads` (wave B, worker **h**; checklist `downloads.page`, `downloads.history`,
//! `downloads.queue`, `downloads.local-library.*`, `downloads.integrity-check`, `downloads.storage-limit`; classic
//! `pages/profile/OfflineLibraryPage.tsx`, `core/download/*`): tabs Active · History · Anime · Manga · Novels · Local
//! library (`role="tab"` + `data-tab`), rows from `Backend::downloads()` (web downloads merged with the torrent
//! client's kept torrents), per-row pause / resume / cancel / retry / verify / play / open folder / remove
//! (`data-download-id`, `data-state`, `data-kind`, `data-progress`), search, storage header (`download_storage`,
//! `data-storage-used`, `data-storage-limit`, `data-storage-limit-reached`), "Check now" of the auto-downloader and
//! the local library: import files, scan a folder, match / fix, play offline, remove (`data-local-id`,
//! `data-matched`). Polled every 2 s while something downloads, else every 6 s.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{
    AutoDownloadRunVm, BackendRef, DownloadStorageVm, DownloadVm, LibraryScanVm, LocalItemVm, MediaVm, UiResult,
};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize,
    LoadingIndicator, Pill, Sheet, SkeletonLines, Switch, TextField, Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::UiError;
use crate::model::downloads::{
    DownloadKindVm, DownloadStateVm, DownloadsTabVm, OPEN_ROOT_FOLDER, filter_rows, format_bytes, format_eta,
    format_percent, format_when, storage_fraction, storage_limit_reached,
};

const POLL_ACTIVE: Duration = Duration::from_secs(2);
const POLL_IDLE: Duration = Duration::from_secs(6);
const IMPORT_INPUT_ID: &str = "tk-downloads-import";
const SCAN_INPUT_ID: &str = "tk-downloads-scan";

/// Pill tone of a row state.
pub fn state_tone(state: DownloadStateVm) -> Tone {
    match state {
        DownloadStateVm::Queued | DownloadStateVm::Paused => Tone::Neutral,
        DownloadStateVm::Downloading => Tone::Primary,
        DownloadStateVm::Completed => Tone::Ok,
        DownloadStateVm::Failed | DownloadStateVm::Broken => Tone::Err,
        DownloadStateVm::Cancelled => Tone::Warn,
    }
}

/// Row ids of a list whose state counts as running (to poll faster).
fn any_running(rows: &[DownloadVm]) -> bool {
    rows.iter().any(|d| matches!(d.state, DownloadStateVm::Downloading | DownloadStateVm::Queued))
}

/// Count shown next to a tab label (`None` for the local tab, it has its own list).
fn tab_count(rows: &[DownloadVm], tab: DownloadsTabVm) -> Option<usize> {
    (tab != DownloadsTabVm::Local).then(|| rows.iter().filter(|d| tab.shows(d)).count())
}

/// What the confirmation dialog is about.
#[derive(Debug, Clone, PartialEq)]
enum Confirm {
    /// Remove a row (`true` = still active: cancel first).
    Row(i64, bool),
    Local(i64),
    ClearHistory,
}

/// Row actions sent up from `DownloadItem`.
#[derive(Debug, Clone, PartialEq)]
enum RowAction {
    Pause,
    Resume,
    Retry,
    Cancel,
    Verify,
    OpenFolder,
    Play,
    Remove,
}

async fn load_rows(backend: &BackendRef) -> UiResult<Vec<DownloadVm>> {
    backend.downloads().await
}

#[component]
pub fn DownloadsPage() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let mut tab = use_signal(DownloadsTabVm::default);
    let mut query = use_signal(String::new);
    let mut rows = use_signal(|| None::<UiResult<Vec<DownloadVm>>>);
    let mut storage = use_signal(|| None::<UiResult<DownloadStorageVm>>);
    let mut local = use_signal(|| None::<UiResult<Vec<LocalItemVm>>>);
    let mut action_error = use_signal(|| None::<UiError>);
    let mut busy = use_signal(|| false);
    let mut confirm = use_signal(|| None::<Confirm>);
    let mut delete_files = use_signal(|| false);
    let mut matching = use_signal(|| None::<LocalItemVm>);
    let mut scan_path = use_signal(String::new);
    let mut scan_result = use_signal(|| None::<LibraryScanVm>);

    let refresh = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                rows.set(Some(load_rows(&backend).await));
                storage.set(Some(backend.download_storage().await));
            });
        }
    });
    let refresh_local = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                local.set(Some(backend.local_library().await));
            });
        }
    });

    // Rows + storage on a timer; the local library once and whenever its tab is shown.
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                let mut ticks: u32 = 0;
                loop {
                    let next = load_rows(&backend).await;
                    let running = matches!(&next, Ok(v) if any_running(v));
                    if (*rows.peek()).as_ref() != Some(&next) {
                        rows.set(Some(next));
                    }
                    if ticks % 5 == 0 || running {
                        let s = backend.download_storage().await;
                        if (*storage.peek()).as_ref() != Some(&s) {
                            storage.set(Some(s));
                        }
                    }
                    if local.peek().is_none() || (*tab.peek() == DownloadsTabVm::Local && ticks % 5 == 0) {
                        let l = backend.local_library().await;
                        if (*local.peek()).as_ref() != Some(&l) {
                            local.set(Some(l));
                        }
                    }
                    ticks = ticks.wrapping_add(1);
                    tokio::time::sleep(if running { POLL_ACTIVE } else { POLL_IDLE }).await;
                }
            }
        }
    });

    let row_action = use_callback({
        let backend = backend.clone();
        move |(d, action): (DownloadVm, RowAction)| {
            if action == RowAction::Play {
                if let Some(ep) = d.episode.filter(|_| !d.media_id.is_empty()) {
                    navigator.push(Route::PlayerPage { id: d.media_id.clone(), episode: ep });
                }
                return;
            }
            if action == RowAction::Remove {
                delete_files.set(false);
                confirm.set(Some(Confirm::Row(d.id, d.state.is_active())));
                return;
            }
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                let r = match action {
                    RowAction::Pause => backend.download_pause(d.id).await,
                    RowAction::Resume | RowAction::Retry => backend.download_resume(d.id).await,
                    RowAction::Cancel => backend.download_cancel(d.id, false).await,
                    RowAction::OpenFolder => backend.download_open_folder(d.id).await,
                    RowAction::Verify => match backend.download_verify(d.id).await {
                        Ok(v) if v.state == DownloadStateVm::Broken => {
                            snack.show(t("downloads.verify.broken"));
                            Ok(())
                        }
                        Ok(_) => {
                            snack.show(t("downloads.verify.ok"));
                            Ok(())
                        }
                        Err(e) => Err(e),
                    },
                    RowAction::Play | RowAction::Remove => Ok(()),
                };
                if let Err(e) = r {
                    action_error.set(Some(e));
                }
                refresh.call(());
            });
        }
    });

    let open_root = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.download_open_folder(OPEN_ROOT_FOLDER).await {
                    action_error.set(Some(e));
                }
            });
        }
    });

    let confirmed = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Some(what) = confirm() else { return };
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let with_files = delete_files();
            let backend = backend.clone();
            spawn(async move {
                let r = match what {
                    Confirm::Row(id, active) => {
                        let cancelled = if active { backend.download_cancel(id, with_files).await } else { Ok(()) };
                        match cancelled {
                            Ok(()) => backend.download_remove(id, with_files).await.map(|_| t("downloads.removed")),
                            Err(e) => Err(e),
                        }
                    }
                    Confirm::Local(id) => backend.local_remove(id, with_files).await.map(|_| t("downloads.local.removed")),
                    Confirm::ClearHistory => {
                        backend.download_clear_history().await.map(|n| tn("downloads.history.cleared", n as usize))
                    }
                };
                match r {
                    Ok(msg) => snack.show(msg),
                    Err(e) => action_error.set(Some(e)),
                }
                confirm.set(None);
                busy.set(false);
                refresh.call(());
                refresh_local.call(());
            });
        }
    });

    let scan = use_callback({
        let backend = backend.clone();
        move |dir: Option<String>| {
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.library_scan(dir.as_deref().map(str::trim).filter(|d| !d.is_empty())).await {
                    Ok(vm) => scan_result.set(Some(vm)),
                    Err(e) => action_error.set(Some(e)),
                }
                busy.set(false);
                refresh_local.call(());
            });
        }
    });

    let import = use_callback({
        let backend = backend.clone();
        move |paths: Vec<String>| {
            if busy() || paths.is_empty() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.library_import(&paths).await {
                    Ok(vm) => scan_result.set(Some(vm)),
                    Err(e) => action_error.set(Some(e)),
                }
                busy.set(false);
                refresh_local.call(());
            });
        }
    });

    let current = tab();
    let storage_head = match &*storage.read() {
        Some(Ok(s)) => rsx! { StorageHeader { storage: s.clone(), on_open: move |_| open_root.call(()) } },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, title: Some(t("downloads.storage.error")), on_retry: move |_| refresh.call(()) }
        },
        None => rsx! { SkeletonLines { widths: vec![60, 40] } },
    };

    let all_rows: Vec<DownloadVm> = match &*rows.read() {
        Some(Ok(v)) => v.clone(),
        _ => Vec::new(),
    };
    let tabs = DownloadsTabVm::ALL.iter().map(|tb| {
        let tb = *tb;
        let count = tab_count(&all_rows, tb);
        let selected = tb == current;
        rsx! {
            button {
                key: "{tb.id()}",
                class: "tk-dtab tk-downloads__tab",
                role: "tab",
                r#type: "button",
                "data-tab": tb.id(),
                "aria-selected": "{selected}",
                tabindex: if selected { "0" } else { "-1" },
                onclick: move |_| {
                    tab.set(tb);
                    if tb == DownloadsTabVm::Local {
                        refresh_local.call(());
                    }
                },
                span { {t(&format!("downloads.tab.{}", tb.id()))} }
                if let Some(n) = count.filter(|n| *n > 0) {
                    span { class: "tk-downloads__tab-count", "{n}" }
                }
            }
        }
    });

    let body = if current == DownloadsTabVm::Local {
        let list = match &*local.read() {
            None => rsx! { div { "aria-busy": "true", SkeletonLines { widths: vec![100, 90, 80] } } },
            Some(Err(e)) => rsx! {
                ErrorState { error: e.clone(), title: Some(t("downloads.local.error")), on_retry: move |_| refresh_local.call(()) }
            },
            Some(Ok(items)) if items.is_empty() => rsx! {
                EmptyState { icon: "folder".to_string(), title: t("downloads.local.empty.title"), body: t("downloads.local.empty.body") }
            },
            Some(Ok(items)) => {
                let q = query().trim().to_lowercase();
                let shown: Vec<LocalItemVm> = items
                    .iter()
                    .filter(|i| {
                        q.is_empty()
                            || i.file_name.to_lowercase().contains(&q)
                            || i.media.as_ref().is_some_and(|m| m.title.to_lowercase().contains(&q))
                    })
                    .cloned()
                    .collect();
                if shown.is_empty() {
                    rsx! { EmptyState { icon: "search".to_string(), title: t("downloads.search.empty") } }
                } else {
                    rsx! {
                        ul { class: "tk-downloads__list", role: "list",
                            for item in shown {
                                LocalRow {
                                    key: "{item.id}",
                                    item: item.clone(),
                                    busy: busy(),
                                    on_play: move |(id, ep): (String, u32)| {
                                        navigator.push(Route::PlayerPage { id, episode: ep });
                                    },
                                    on_match: move |it: LocalItemVm| matching.set(Some(it)),
                                    on_remove: move |id: i64| {
                                        delete_files.set(false);
                                        confirm.set(Some(Confirm::Local(id)));
                                    },
                                }
                            }
                        }
                    }
                }
            }
        };
        rsx! {
            div { class: "tk-downloads__local-bar",
                TextField {
                    label: t("downloads.local.scan_path"),
                    value: scan_path(),
                    icon: "folder".to_string(),
                    mono: true,
                    hint: t("downloads.local.scan_hint"),
                    disabled: busy(),
                    oninput: move |v: String| scan_path.set(v),
                    onsubmit: move |_| scan.call(Some(scan_path.peek().clone())),
                }
                Button {
                    label: t("downloads.local.scan"),
                    icon: "refresh".to_string(),
                    variant: ButtonVariant::Tonal,
                    disabled: busy(),
                    onclick: move |_| {
                        let p = scan_path.peek().clone();
                        scan.call(Some(p).filter(|p| !p.trim().is_empty()));
                    },
                }
                Button {
                    label: t("downloads.local.pick_folder"),
                    icon: "folder".to_string(),
                    variant: ButtonVariant::Outlined,
                    disabled: busy(),
                    onclick: move |_| {
                        let _ = document::eval(&format!("const i=document.getElementById('{SCAN_INPUT_ID}');if(i){{i.click();}}"));
                    },
                }
                Button {
                    label: t("downloads.local.import"),
                    icon: "plus".to_string(),
                    variant: ButtonVariant::Outlined,
                    disabled: busy(),
                    onclick: move |_| {
                        let _ = document::eval(&format!("const i=document.getElementById('{IMPORT_INPUT_ID}');if(i){{i.click();}}"));
                    },
                }
                input {
                    id: "{SCAN_INPUT_ID}",
                    r#type: "file",
                    directory: true,
                    hidden: true,
                    onchange: move |e| {
                        if let Some(f) = e.files().first() {
                            let p = f.path().to_string_lossy().into_owned();
                            scan_path.set(p.clone());
                            scan.call(Some(p));
                        }
                    },
                }
                input {
                    id: "{IMPORT_INPUT_ID}",
                    r#type: "file",
                    multiple: true,
                    accept: "video/*,.mkv,.mp4,.avi,.webm,.ts,.m4v,.mov",
                    hidden: true,
                    onchange: move |e| {
                        let paths: Vec<String> = e.files().iter().map(|f| f.path().to_string_lossy().into_owned()).collect();
                        import.call(paths);
                    },
                }
                if busy() {
                    LoadingIndicator { small: true }
                }
            }
            if let Some(r) = scan_result() {
                Banner {
                    text: tf(
                        "downloads.local.scan_result",
                        &[
                            ("scanned", &r.scanned.to_string()),
                            ("added", &r.added.to_string()),
                            ("matched", &r.matched.to_string()),
                        ],
                    ),
                    icon: "check".to_string(),
                    tone: if r.unmatched.is_empty() { Tone::Ok } else { Tone::Warn },
                    if !r.unmatched.is_empty() {
                        span { class: "tk-downloads__unmatched", {tn("downloads.local.unmatched", r.unmatched.len())} }
                    }
                }
            }
            {list}
        }
    } else {
        match &*rows.read() {
            None => rsx! { div { "aria-busy": "true", SkeletonLines { widths: vec![100, 90, 80] } } },
            Some(Err(e)) => rsx! {
                ErrorState { error: e.clone(), title: Some(t("downloads.error")), on_retry: move |_| refresh.call(()) }
            },
            Some(Ok(list)) => {
                let shown: Vec<DownloadVm> = filter_rows(list, current, &query()).into_iter().cloned().collect();
                if shown.is_empty() {
                    let searching = !query().trim().is_empty();
                    let (title, body) = if searching {
                        (t("downloads.search.empty"), String::new())
                    } else {
                        (t(&format!("downloads.empty.{}.title", current.id())), t(&format!("downloads.empty.{}.body", current.id())))
                    };
                    // UX review #6: offer the next step (find something to download) instead of a dead end.
                    let action = if searching { String::new() } else { t("downloads.empty.action") };
                    rsx! {
                        EmptyState {
                            icon: "download".to_string(),
                            title,
                            body,
                            action_label: action,
                            on_action: move |_| {
                                navigator.push(Route::SearchPage {});
                            },
                        }
                    }
                } else {
                    rsx! {
                        if current == DownloadsTabVm::History {
                            div { class: "tk-downloads__history-bar",
                                Button {
                                    label: t("downloads.history.clear"),
                                    icon: "trash".to_string(),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: busy(),
                                    onclick: move |_| {
                                        delete_files.set(false);
                                        confirm.set(Some(Confirm::ClearHistory));
                                    },
                                }
                            }
                        }
                        ul { class: "tk-downloads__list", role: "list",
                            for d in shown {
                                DownloadItem {
                                    key: "{d.id}",
                                    item: d.clone(),
                                    history: current == DownloadsTabVm::History,
                                    on_action: move |a: (DownloadVm, RowAction)| row_action.call(a),
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    let dialog = confirm().map(|what| {
        let (title, body, label, files_switch) = match what {
            Confirm::Row(_, true) => (t("downloads.cancel.title"), t("downloads.cancel.body"), t("downloads.cancel"), true),
            Confirm::Row(_, false) => (t("downloads.remove.title"), t("downloads.remove.body"), t("common.remove"), true),
            Confirm::Local(_) => (t("downloads.local.remove.title"), t("downloads.local.remove.body"), t("common.remove"), true),
            Confirm::ClearHistory => {
                (t("downloads.history.clear_title"), t("downloads.history.clear_body"), t("downloads.history.clear"), false)
            }
        };
        rsx! {
            ConfirmDialog {
                title: title,
                body: body,
                confirm_label: label,
                icon: "trash".to_string(),
                confirm_disabled: busy(),
                on_confirm: move |_| confirmed.call(()),
                on_cancel: move |_| {
                    if !busy() {
                        confirm.set(None);
                    }
                },
                if files_switch {
                    div { class: "tk-downloads__files-opt",
                        Switch {
                            checked: delete_files(),
                            label: t("downloads.delete_files"),
                            on_toggle: move |v: bool| delete_files.set(v),
                        }
                        span { {t("downloads.delete_files")} }
                    }
                }
            }
        }
    });

    let match_sheet = matching().map(|item| {
        rsx! {
            LocalMatchSheet {
                item: item,
                on_close: move |done: bool| {
                    matching.set(None);
                    if done {
                        snack.show(t("downloads.local.matched"));
                        refresh_local.call(());
                    }
                },
            }
        }
    });

    rsx! {
        div { class: "tk-page tk-downloads", "data-page": "downloads.title",
            div { class: "tk-page__head tk-downloads__head",
                h1 { class: "tk-page__title", {t("downloads.title")} }
                div { class: "tk-downloads__head-actions",
                    // The torrent client has no rail entry since the UX review (#1): it opens from here.
                    Button {
                        label: t("downloads.torrent_client"),
                        icon: "magnet".to_string(),
                        variant: ButtonVariant::Outlined,
                        onclick: move |_| {
                            navigator.push(Route::TorrentOverviewPage {});
                        },
                    }
                    AutoCheckNow { on_done: move |_| refresh.call(()) }
                    Button {
                        label: t("downloads.open_folder"),
                        icon: "folder".to_string(),
                        variant: ButtonVariant::Tonal,
                        onclick: move |_| open_root.call(()),
                    }
                }
            }
            {storage_head}
            div { class: "tk-downloads__toolbar",
                div { class: "tk-dtabs tk-downloads__tabs", role: "tablist", "aria-label": t("downloads.title"), {tabs} }
                div { class: "tk-downloads__search",
                    TextField {
                        label: t("downloads.search"),
                        value: query(),
                        icon: "search".to_string(),
                        oninput: move |v: String| query.set(v),
                    }
                }
            }
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false, title: Some(t("downloads.action_failed")),
                    Button { label: t("common.close"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| action_error.set(None) }
                }
            }
            div { class: "tk-downloads__panel", role: "tabpanel", "data-tab-panel": current.id(), {body} }
        }
        {dialog}
        {match_sheet}
    }
}

/// Folder, disk use and the storage cap (`downloads.storage-limit`).
#[component]
fn StorageHeader(storage: DownloadStorageVm, on_open: EventHandler<()>) -> Element {
    let reached = storage_limit_reached(storage.used_bytes, storage.limit_bytes);
    let used = format_bytes(storage.used_bytes);
    let line = match storage.limit_bytes {
        Some(l) => tf("downloads.storage.used_of", &[("used", &used), ("limit", &format_bytes(l))]),
        None => tf("downloads.storage.used", &[("used", &used)]),
    };
    let free = storage.free_bytes.map(|f| tf("downloads.storage.free", &[("free", &format_bytes(f))]));
    let items = tn("downloads.storage.items", storage.items as usize);
    let limit_attr = storage.limit_bytes.map(|l| l.to_string()).unwrap_or_default();
    rsx! {
        div {
            class: "tk-downloads__storage",
            "data-storage-used": "{storage.used_bytes}",
            "data-storage-limit": "{limit_attr}",
            "data-storage-limit-reached": "{reached}",
            div { class: "tk-downloads__storage-text",
                button {
                    class: "tk-downloads__dir",
                    r#type: "button",
                    title: t("downloads.open_folder"),
                    onclick: move |_| on_open.call(()),
                    Icon { name: "folder".to_string(), size: 18 }
                    span { class: "tk-downloads__dir-path", "{storage.dir}" }
                }
                span { class: "tk-downloads__storage-line", "{line} · {items}" }
                if let Some(f) = free {
                    span { class: "tk-downloads__storage-free", "{f}" }
                }
            }
            if let Some(frac) = storage_fraction(&storage) {
                WavyProgress { fraction: frac, label: line.clone(), active: false, thin: true }
            }
            if reached {
                Banner { text: t("downloads.storage.reached"), icon: "warning".to_string(), tone: Tone::Warn }
            }
        }
    }
}

/// "Check now" of the auto-downloader (`downloads.auto.profiles`, `data-auto-download` + `data-checked` /
/// `data-enqueued` after a pass). Shared by the page head and Settings > Downloads.
#[component]
pub fn AutoCheckNow(#[props(default)] on_done: EventHandler<AutoDownloadRunVm>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut busy = use_signal(|| false);
    let mut last = use_signal(|| None::<AutoDownloadRunVm>);
    let mut error = use_signal(|| None::<UiError>);
    let run = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.auto_download_check_now().await {
                    Ok(r) => {
                        snack.show(tf(
                            "downloads.auto.result",
                            &[("checked", &r.checked.to_string()), ("enqueued", &r.enqueued.to_string())],
                        ));
                        last.set(Some(r.clone()));
                        on_done.call(r);
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });
    let (checked, enqueued) = last().map(|r| (r.checked.to_string(), r.enqueued.to_string())).unwrap_or_default();
    rsx! {
        div {
            class: "tk-downloads__autocheck",
            "data-auto-download": "",
            "data-checked": "{checked}",
            "data-enqueued": "{enqueued}",
            "data-busy": "{busy()}",
            Button {
                label: t("downloads.auto.check_now"),
                icon: "sync".to_string(),
                variant: ButtonVariant::Outlined,
                disabled: busy(),
                onclick: move |_| run.call(()),
            }
            if busy() {
                LoadingIndicator { small: true }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, on_retry: move |_| run.call(()) }
            }
        }
    }
}

#[component]
fn DownloadItem(item: DownloadVm, history: bool, on_action: EventHandler<(DownloadVm, RowAction)>) -> Element {
    let d = item;
    let actions = d.state.actions();
    let state_text = t(&format!("downloads.state.{}", d.state.id()));
    let kind_text = t(&format!("downloads.kind.{}", d.kind.id()));
    let sub = match (d.episode, d.chapter.as_deref()) {
        (Some(e), _) => tf("downloads.episode", &[("n", &e.to_string())]),
        (None, Some(c)) => c.to_owned(),
        _ => String::new(),
    };
    let pct = format_percent(d.progress);
    let sizes = match d.bytes_total {
        Some(total) if total > 0 => {
            tf("downloads.of", &[("done", &format_bytes(d.bytes_done.min(total))), ("total", &format_bytes(total))])
        }
        _ => format_bytes(d.bytes_done),
    };
    let speed = (d.state == DownloadStateVm::Downloading && d.speed_bps > 0)
        .then(|| tf("downloads.speed", &[("speed", &format_bytes(d.speed_bps))]));
    let eta = d.eta_secs.filter(|_| d.state == DownloadStateVm::Downloading).map(|s| tf("downloads.eta", &[("time", &format_eta(s))]));
    let when = if history {
        let at = d.completed_unix.unwrap_or(d.added_unix);
        Some(format_when(at)).filter(|w| !w.is_empty())
    } else {
        None
    };
    let playable = actions.play && d.kind.is_episode() && d.episode.is_some() && !d.media_id.is_empty();
    let progress_attr = format!("{:.4}", d.progress);
    let a = d.clone();
    let act = move |action: RowAction| on_action.call((a.clone(), action));
    let (a1, a2, a3, a4, a5, a6, a7, a8) = (act.clone(), act.clone(), act.clone(), act.clone(), act.clone(), act.clone(), act.clone(), act);
    rsx! {
        li {
            class: "tk-downloads__item",
            "data-download-id": "{d.id}",
            "data-state": d.state.id(),
            "data-kind": d.kind.id(),
            "data-progress": "{progress_attr}",
            div { class: "tk-downloads__thumb",
                if let Some(p) = d.poster.clone() {
                    img { src: "{p}", alt: "", loading: "lazy" }
                } else {
                    Icon { name: if d.kind.is_episode() { "film".to_string() } else { "bookmark".to_string() }, size: 24 }
                }
            }
            div { class: "tk-downloads__main",
                div { class: "tk-downloads__item-head",
                    div { class: "tk-downloads__titles",
                        span { class: "tk-downloads__item-title", title: "{d.title}", "{d.title}" }
                        if !sub.is_empty() {
                            span { class: "tk-downloads__item-sub", "{sub}" }
                        }
                    }
                    div { class: "tk-downloads__pills",
                        Pill { text: kind_text, plain: true }
                        Pill { text: state_text, tone: state_tone(d.state) }
                    }
                }
                if d.state != DownloadStateVm::Completed {
                    div { class: "tk-downloads__progress",
                        WavyProgress {
                            fraction: d.progress,
                            label: tf("downloads.progress", &[("pct", &pct)]),
                            active: d.state == DownloadStateVm::Downloading,
                        }
                        span { class: "tk-downloads__pct", "{pct}" }
                    }
                }
                div { class: "tk-downloads__stats",
                    span { class: "tk-downloads__stat", "{sizes}" }
                    if let Some(s) = speed {
                        span { class: "tk-downloads__stat", Icon { name: "download".to_string(), size: 16 } "{s}" }
                    }
                    if let Some(e) = eta {
                        span { class: "tk-downloads__stat", Icon { name: "clock".to_string(), size: 16 } "{e}" }
                    }
                    if let Some(w) = when {
                        span { class: "tk-downloads__stat", Icon { name: "calendar".to_string(), size: 16 } "{w}" }
                    }
                }
                if d.state == DownloadStateVm::Broken {
                    p { class: "tk-downloads__item-error", role: "alert", {t("downloads.broken.body")} }
                } else if let Some(err) = d.error.clone() {
                    p { class: "tk-downloads__item-error", role: "alert", "{err}" }
                }
                div { class: "tk-downloads__actions",
                    if playable {
                        Button { label: t("downloads.play"), icon: "play".to_string(), size: ButtonSize::Sm, onclick: move |_| a1(RowAction::Play) }
                    }
                    if actions.pause {
                        Button { label: t("downloads.pause"), icon: "pause".to_string(), variant: ButtonVariant::Tonal, size: ButtonSize::Sm, onclick: move |_| a2(RowAction::Pause) }
                    }
                    if actions.resume {
                        Button { label: t("downloads.resume"), icon: "play".to_string(), variant: ButtonVariant::Tonal, size: ButtonSize::Sm, onclick: move |_| a3(RowAction::Resume) }
                    }
                    if actions.retry && d.kind != DownloadKindVm::Torrent {
                        Button {
                            label: if d.state == DownloadStateVm::Broken { t("downloads.redownload") } else { t("downloads.retry") },
                            icon: "replay".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            onclick: move |_| a4(RowAction::Retry),
                        }
                    }
                    if actions.cancel {
                        Button { label: t("downloads.cancel"), icon: "close".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| a5(RowAction::Cancel) }
                    }
                    if actions.verify && d.kind != DownloadKindVm::Torrent {
                        Button { label: t("downloads.verify"), icon: "shield".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| a6(RowAction::Verify) }
                    }
                    IconButton { icon: "folder".to_string(), label: t("downloads.open_folder"), size: IconButtonSize::Sm, onclick: move |_| a7(RowAction::OpenFolder) }
                    IconButton { icon: "trash".to_string(), label: t("common.remove"), size: IconButtonSize::Sm, onclick: move |_| a8(RowAction::Remove) }
                }
            }
        }
    }
}

#[component]
fn LocalRow(
    item: LocalItemVm,
    busy: bool,
    on_play: EventHandler<(String, u32)>,
    on_match: EventHandler<LocalItemVm>,
    on_remove: EventHandler<i64>,
) -> Element {
    let matched = item.media.is_some();
    let target = item.media.as_ref().map(|m| m.id.clone()).zip(item.episode);
    let label = match (&item.media, item.episode) {
        (Some(m), Some(e)) => tf("downloads.local.match_line", &[("title", &m.title), ("n", &e.to_string())]),
        (Some(m), None) => m.title.clone(),
        _ => t("downloads.local.unmatched_one"),
    };
    let size = format_bytes(item.size_bytes);
    let (id, it) = (item.id, item.clone());
    rsx! {
        li {
            class: "tk-downloads__item tk-downloads__item--local",
            "data-local-id": "{item.id}",
            "data-matched": "{matched}",
            "data-playable": "{item.playable}",
            div { class: "tk-downloads__thumb",
                if let Some(p) = item.media.as_ref().and_then(|m| m.cover.clone()) {
                    img { src: "{p}", alt: "", loading: "lazy" }
                } else {
                    Icon { name: "film".to_string(), size: 24 }
                }
            }
            div { class: "tk-downloads__main",
                div { class: "tk-downloads__item-head",
                    div { class: "tk-downloads__titles",
                        span { class: "tk-downloads__item-title", title: "{item.path}", "{item.file_name}" }
                        span { class: "tk-downloads__item-sub", "{label}" }
                    }
                    div { class: "tk-downloads__pills",
                        if !item.playable {
                            Pill { text: t("downloads.local.missing"), tone: Tone::Err }
                        } else if matched {
                            Pill { text: t("downloads.local.matched_pill"), tone: Tone::Ok }
                        } else {
                            Pill { text: t("downloads.local.unmatched_pill"), tone: Tone::Warn }
                        }
                    }
                }
                div { class: "tk-downloads__stats",
                    span { class: "tk-downloads__stat", "{size}" }
                    span { class: "tk-downloads__stat tk-downloads__path", "{item.path}" }
                }
                div { class: "tk-downloads__actions",
                    if let Some((mid, ep)) = target.clone().filter(|_| item.playable) {
                        Button {
                            label: t("downloads.play"),
                            icon: "play".to_string(),
                            size: ButtonSize::Sm,
                            onclick: move |_| on_play.call((mid.clone(), ep)),
                        }
                    }
                    Button {
                        label: if matched { t("downloads.local.fix") } else { t("downloads.local.match") },
                        icon: "edit".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: busy,
                        onclick: move |_| on_match.call(it.clone()),
                    }
                    IconButton { icon: "trash".to_string(), label: t("common.remove"), size: IconButtonSize::Sm, disabled: busy, onclick: move |_| on_remove.call(id) }
                }
            }
        }
    }
}

/// Text a file name suggests for the title search (`[Group] Show - 01 (1080p).mkv` -> `Show`).
pub fn search_hint(file_name: &str) -> String {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(s, _)| s);
    let mut out = String::new();
    let mut depth = 0i32;
    for c in stem.chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(if c == '_' || c == '.' { ' ' } else { c }),
            _ => {}
        }
    }
    let cut = out.find(" - ").map_or(out.as_str(), |i| &out[..i]);
    let words: Vec<&str> = cut
        .split_whitespace()
        .take_while(|w| !is_episode_token(w))
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .collect();
    words.join(" ")
}

/// `S02E05`, `E12`, `s1`: where the title part of a release name ends.
fn is_episode_token(w: &str) -> bool {
    let l = w.to_lowercase();
    let mut cs = l.chars();
    match cs.next() {
        Some('s' | 'e') => {
            let rest: String = cs.collect();
            rest.starts_with(|c: char| c.is_ascii_digit()) && rest.chars().all(|c| c.is_ascii_digit() || c == 'e')
        }
        _ => false,
    }
}

/// Pick the title (search) and episode of a local file (`downloads.local-library.match`).
#[component]
fn LocalMatchSheet(item: LocalItemVm, on_close: EventHandler<bool>) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let initial = item.media.as_ref().map(|m| m.title.clone()).unwrap_or_else(|| search_hint(&item.file_name));
    let mut query = use_signal(|| initial);
    let mut results = use_signal(|| None::<UiResult<Vec<MediaVm>>>);
    let mut chosen = use_signal(|| item.media.clone());
    let mut episode = use_signal(|| item.episode.map(|e| e.to_string()).unwrap_or_default());
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let search = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let q = query.peek().trim().to_owned();
            if q.is_empty() {
                return;
            }
            results.set(None);
            let backend = backend.clone();
            spawn(async move {
                results.set(Some(backend.search(&q, 1).await));
            });
        }
    });
    use_hook(|| search.call(()));

    let save = use_callback({
        let backend = backend.clone();
        let id = item.id;
        move |_: ()| {
            let Some(m) = chosen() else { return };
            let Ok(ep) = episode.peek().trim().parse::<u32>() else { return };
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.local_match(id, &m.id, ep).await {
                    Ok(()) => on_close.call(true),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let ep_valid = episode().trim().parse::<u32>().is_ok_and(|e| e > 0);
    let list = match &*results.read() {
        None => rsx! { div { class: "tk-downloads__match-loading", LoadingIndicator {} } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| search.call(()) } },
        Some(Ok(v)) if v.is_empty() => rsx! { EmptyState { icon: "search".to_string(), title: t("downloads.local.match_empty") } },
        Some(Ok(v)) => {
            let current = chosen().map(|m| m.id);
            let options: Vec<(MediaVm, bool)> =
                v.iter().map(|m| (m.clone(), current.as_deref() == Some(m.id.as_str()))).collect();
            rsx! {
                ul { class: "tk-downloads__match-list", role: "list",
                    for (m, pressed) in options {
                        li { key: "{m.id}",
                            button {
                                class: "tk-downloads__match-item",
                                r#type: "button",
                                "aria-pressed": "{pressed}",
                                "data-media-id": "{m.id}",
                                onclick: {
                                    let m = m.clone();
                                    move |_| chosen.set(Some(m.clone()))
                                },
                                if let Some(c) = m.cover.clone() {
                                    img { src: "{c}", alt: "", loading: "lazy" }
                                }
                                span { class: "tk-downloads__match-title", "{m.title}" }
                                if !m.subtitle.is_empty() {
                                    span { class: "tk-downloads__match-sub", "{m.subtitle}" }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        Sheet {
            title: t("downloads.local.match_title"),
            on_close: move |_| on_close.call(false),
            footer: rsx! {
                Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_close.call(false) }
                Button {
                    label: t("common.save"),
                    icon: "check".to_string(),
                    disabled: busy() || chosen().is_none() || !ep_valid,
                    onclick: move |_| save.call(()),
                }
            },
            div { class: "tk-downloads__match",
                p { class: "tk-downloads__match-file", "{item.file_name}" }
                div { class: "tk-downloads__match-search",
                    TextField {
                        label: t("downloads.local.match_search"),
                        value: query(),
                        icon: "search".to_string(),
                        oninput: move |v: String| query.set(v),
                        onsubmit: move |_| search.call(()),
                    }
                    Button { label: t("downloads.search"), variant: ButtonVariant::Tonal, onclick: move |_| search.call(()) }
                }
                {list}
                TextField {
                    label: t("downloads.local.match_episode"),
                    value: episode(),
                    kind: "number".to_string(),
                    error: !episode().trim().is_empty() && !ep_valid,
                    oninput: move |v: String| episode.set(v),
                }
                if let Some(e) = error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_from_file_names() {
        assert_eq!(search_hint("[SubsPlease] Sousou no Frieren - 01 (1080p) [ABCD].mkv"), "Sousou no Frieren");
        assert_eq!(search_hint("Show.Name.S02E05.mp4"), "Show Name");
        assert_eq!(search_hint("One_Piece_1071.mkv"), "One Piece");
    }

    #[test]
    fn tones_and_counts() {
        assert_eq!(state_tone(DownloadStateVm::Broken), Tone::Err);
        assert_eq!(state_tone(DownloadStateVm::Completed), Tone::Ok);
        let row = DownloadVm {
            id: 1,
            media_id: "1".into(),
            title: "x".into(),
            poster: None,
            episode: Some(1),
            chapter: None,
            kind: DownloadKindVm::Hls,
            state: DownloadStateVm::Queued,
            progress: 0.0,
            bytes_done: 0,
            bytes_total: None,
            speed_bps: 0,
            eta_secs: None,
            error: None,
            path: None,
            added_unix: 0,
            completed_unix: None,
        };
        let rows = vec![row.clone(), DownloadVm { id: 2, state: DownloadStateVm::Completed, ..row }];
        assert!(any_running(&rows));
        assert_eq!(tab_count(&rows, DownloadsTabVm::Active), Some(1));
        assert_eq!(tab_count(&rows, DownloadsTabVm::History), Some(1));
        assert_eq!(tab_count(&rows, DownloadsTabVm::Local), None);
    }
}
