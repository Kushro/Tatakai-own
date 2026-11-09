//! Torrent client > Library: every torrent of the session (saved + in progress, `torrent_library`, newest first)
//! as a dense sortable table: series / episode, files, size, download state, date added, "keep after watching".
//! Multi-select with a bulk delete (with or without the downloaded files); per row: open in the client (sets
//! `Ui::torrent` and goes to Overview), open the data folder, delete. Polled every 2 s while mounted; narrow
//! windows turn the rows into cards (CSS). Owner: wave 3 D.
//!
//! Also home of the byte / speed / time formatters of the D pages (`downloads.rs` uses them).
//!
//! Wave A (worker i; checklist `torrent.add-magnet`, `torrent.add-file`, `torrent.episode-alignment`,
//! `torrent.magnet-link-handler`): "Add torrent" opens [`AddTorrentSheet`]: a magnet / info-hash / `.torrent` URL or
//! path typed or pasted, or a `.torrent` file chosen / dropped -> `Backend::torrent_preview` -> an editable file <->
//! episode table with the suggested alignment and its confidence (`data-alignment-confidence`, rows
//! `data-file-episode`) -> `torrent_add_manual`. On mount the page consumes `Backend::take_pending_magnet` (a link
//! the OS handed to the app) and opens the sheet with it (`data-pending-magnet="true"`); [`use_pending_magnet_watch`]
//! (mounted by the app shell) does the same from any page while the app is open: it brings the window forward and
//! navigates here. Torrents added by hand show `data-manual="true"`.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{TorrentLibraryItemVm, TorrentPreviewVm, TorrentStateVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize, LoadingIndicator,
    Pill, Sheet, SkeletonLines, Switch, TextField, Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{UiError, format_unix_utc};

/// Library / Downloads refresh period while the page is open.
pub(super) const POLL_SECS: u64 = 2;

/// A `magnet:` link waiting for the "Add torrent" sheet (set by [`use_pending_magnet_watch`]).
static PENDING_ADD: GlobalSignal<Option<String>> = Signal::global(|| None);
/// Torrents added by hand in this run (rows without a title are always manual).
static MANUAL_IDS: GlobalSignal<BTreeSet<String>> = Signal::global(BTreeSet::new);
/// How often the shell asks for a link the OS handed to the app.
const PENDING_POLL_SECS: u64 = 2;
const TORRENT_INPUT_ID: &str = "tk-add-torrent-file";

/// Mounted once by the app shell: polls `Backend::take_pending_magnet` (only once the first-run wizard is done) and,
/// when a link arrives, brings the window forward and opens Torrents > Library with the "Add torrent" sheet.
pub fn use_pending_magnet_watch() {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    use_future(move || {
        let backend = services.backend.clone();
        async move {
            loop {
                let ready = *ui.settings_loaded.peek() && ui.settings.peek().system.setup_done;
                if ready && let Ok(Some(link)) = backend.take_pending_magnet().await {
                    *PENDING_ADD.write() = Some(link);
                    let window = dioxus::desktop::window();
                    window.window.set_visible(true);
                    window.window.set_minimized(false);
                    window.window.set_focus();
                    navigator.push(Route::TorrentLibraryPage {});
                }
                tokio::time::sleep(Duration::from_secs(PENDING_POLL_SECS)).await;
            }
        }
    });
}

/// One row of the alignment table.
#[derive(Debug, Clone, PartialEq)]
struct AlignRow {
    index: usize,
    path: String,
    name: String,
    size: u64,
    video: bool,
    include: bool,
    /// Episode as typed; empty = selected without an episode.
    episode: String,
}

fn is_video_name(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mkv" | "mp4" | "avi" | "webm" | "m2ts" | "ts" | "mov" | "wmv" | "flv")
}

/// Table rows from a preview: video files included, with the suggested episode (or the parsed one).
fn align_rows(p: &TorrentPreviewVm) -> Vec<AlignRow> {
    let suggested: BTreeMap<usize, u32> = p.suggested.iter().copied().collect();
    let mut rows: Vec<AlignRow> = p
        .files
        .iter()
        .map(|f| {
            let video = is_video_name(&f.name);
            AlignRow {
                index: f.index,
                path: f.path.clone(),
                name: f.name.clone(),
                size: f.size_bytes,
                video,
                include: video,
                episode: suggested.get(&f.index).map(|e| e.to_string()).unwrap_or_default(),
            }
        })
        .collect();
    rows.sort_by(|a, b| b.video.cmp(&a.video).then_with(|| a.path.to_lowercase().cmp(&b.path.to_lowercase())));
    rows
}

/// The alignment to send: included rows as `(index, episode)` (episode 0 = no episode). `Err` with the file name
/// whose episode is not a number, or `Err("")` when nothing is included.
fn alignment_of(rows: &[AlignRow]) -> Result<Vec<(usize, u32)>, String> {
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.include) {
        let text = r.episode.trim();
        let episode = if text.is_empty() { 0 } else { text.parse::<u32>().map_err(|_| r.name.clone())? };
        out.push((r.index, episode));
    }
    if out.is_empty() { Err(String::new()) } else { Ok(out) }
}

/// The same episode on two included files.
fn duplicate_episode(rows: &[AlignRow]) -> Option<u32> {
    let mut seen = BTreeSet::new();
    rows.iter()
        .filter(|r| r.include)
        .filter_map(|r| r.episode.trim().parse::<u32>().ok())
        .filter(|e| *e > 0)
        .find(|e| !seen.insert(*e))
}

/// `"21"` (AniList id) or empty; anything else is invalid.
fn parse_media(text: &str) -> Result<Option<String>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if text.chars().all(|c| c.is_ascii_digit()) && text.len() <= 9 { Ok(Some(text.to_owned())) } else { Err(()) }
}

/// `1.38 GB` style (SI units, three significant digits).
pub(super) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// `4.20 MB/s`.
pub(super) fn format_speed(bps: u64) -> String {
    format!("{}/s", format_bytes(bps))
}

/// `42%`, `99.9%` (one decimal between 99 and 100 so "almost done" never reads as done).
pub(super) fn format_percent(fraction: f64) -> String {
    let pct = fraction.clamp(0.0, 1.0) * 100.0;
    if (99.0..100.0).contains(&pct) { format!("{:.1}%", (pct * 10.0).floor() / 10.0) } else { format!("{pct:.0}%") }
}

/// `1h 02m`, `3m 05s`, `45s`.
pub(super) fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs / 60) % 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// `YYYY-MM-DD` of a Unix time (UTC); empty for 0 / unknown.
pub(super) fn format_date(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    format_unix_utc(unix).get(..10).unwrap_or_default().to_owned()
}

/// Pill tone of a torrent state.
pub(super) fn state_tone(state: TorrentStateVm) -> Tone {
    match state {
        TorrentStateVm::Initializing => Tone::Warn,
        TorrentStateVm::Downloading => Tone::Primary,
        TorrentStateVm::Seeding => Tone::Ok,
        TorrentStateVm::Paused => Tone::Neutral,
        TorrentStateVm::Error => Tone::Err,
    }
}

/// Series title, or the torrent name when the torrent was not added from a title.
pub(super) fn display_title(item: &TorrentLibraryItemVm) -> String {
    item.series.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| item.name.clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LibSort {
    Name,
    Episode,
    Size,
    Progress,
    Added,
}

/// Sorted copy of `items` (stable; `desc` reverses). Name compares case-insensitively; missing episodes go last.
fn sort_items(items: &[TorrentLibraryItemVm], by: LibSort, desc: bool) -> Vec<TorrentLibraryItemVm> {
    let mut out = items.to_vec();
    out.sort_by(|a, b| {
        let ord = match by {
            LibSort::Name => display_title(a).to_lowercase().cmp(&display_title(b).to_lowercase()),
            LibSort::Episode => a.episode.unwrap_or(u32::MAX).cmp(&b.episode.unwrap_or(u32::MAX)),
            LibSort::Size => a.size_bytes.cmp(&b.size_bytes),
            LibSort::Progress => a.progress.total_cmp(&b.progress),
            LibSort::Added => a.added_unix.cmp(&b.added_unix),
        };
        if desc { ord.reverse() } else { ord }
    });
    out
}

/// Selected ids that are still in the list (rows removed elsewhere drop out of the selection).
fn prune_selection(selected: &BTreeSet<String>, items: &[TorrentLibraryItemVm]) -> BTreeSet<String> {
    selected.iter().filter(|id| items.iter().any(|i| &i.id == *id)).cloned().collect()
}

#[component]
pub fn TorrentLibraryPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let mut items = use_signal(|| None::<UiResult<Vec<TorrentLibraryItemVm>>>);
    let mut selected = use_signal(BTreeSet::<String>::new);
    let mut sort = use_signal(|| (LibSort::Added, true));
    // Ids waiting for the delete confirmation, and whether their files go too.
    let mut confirm = use_signal(|| None::<Vec<String>>);
    let mut delete_files = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut action_error = use_signal(|| None::<UiError>);
    // "Add torrent" sheet: `Some(initial source)` while open; whether it came from the OS.
    let mut adding = use_signal(|| None::<String>);
    let mut from_os = use_signal(|| false);

    // A link the OS handed to the app (cold start: still in the backend; app open: the shell's watcher).
    use_hook({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            spawn(async move {
                if let Ok(Some(link)) = backend.take_pending_magnet().await {
                    *PENDING_ADD.write() = Some(link);
                }
            });
        }
    });
    use_effect(move || {
        let pending = PENDING_ADD.read().clone();
        if let Some(link) = pending {
            *PENDING_ADD.write() = None;
            from_os.set(true);
            adding.set(Some(link));
        }
    });

    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                loop {
                    let next = backend.torrent_library().await;
                    if (*items.peek()).as_ref() != Some(&next) {
                        if let Ok(list) = &next {
                            let pruned = prune_selection(&selected.peek(), list);
                            if *selected.peek() != pruned {
                                selected.set(pruned);
                            }
                        }
                        items.set(Some(next));
                    }
                    tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
                }
            }
        }
    });

    let refresh = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                items.set(Some(backend.torrent_library().await));
            });
        }
    });

    let set_persist = use_callback({
        let backend = backend.clone();
        move |(id, keep): (String, bool)| {
            let backend = backend.clone();
            spawn(async move {
                match backend.torrent_set_persist(&id, keep).await {
                    Ok(()) => snack.show(t(if keep { "torrent.library.kept" } else { "torrent.library.not_kept" })),
                    Err(e) => action_error.set(Some(e)),
                }
                refresh.call(());
            });
        }
    });

    let open_folder = use_callback({
        let backend = backend.clone();
        move |id: String| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.torrent_open_folder(&id).await {
                    action_error.set(Some(e));
                }
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
            let Some(ids) = confirm() else { return };
            if busy() {
                return;
            }
            let with_files = delete_files();
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                let mut failed = None;
                let mut removed = 0usize;
                for id in &ids {
                    match backend.torrent_remove(id, with_files).await {
                        Ok(()) => removed += 1,
                        Err(e) => failed = Some(e),
                    }
                }
                if removed > 0 {
                    snack.show(tf("torrent.library.removed", &[("n", &removed.to_string())]));
                }
                action_error.set(failed);
                selected.set(BTreeSet::new());
                confirm.set(None);
                busy.set(false);
                refresh.call(());
            });
        }
    });

    let body = match &*items.read() {
        None => rsx! {
            div { class: "tk-tlib__loading", "aria-busy": "true",
                SkeletonLines { widths: vec![100, 92, 86, 78, 70] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: Some(t("torrent.library.error")), on_retry: move |_| refresh.call(()) }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState {
                icon: "magnet".to_string(),
                title: t("torrent.library.empty.title"),
                body: t("torrent.library.empty.body"),
                action_label: t("torrent.add.open"),
                on_action: move |_| adding.set(Some(String::new())),
            }
        },
        Some(Ok(list)) => {
            let current = sort();
            let rows = sort_items(list, current.0, current.1);
            let sel = selected();
            let all_ids: BTreeSet<String> = list.iter().map(|i| i.id.clone()).collect();
            let all_selected = !all_ids.is_empty() && all_ids.iter().all(|id| sel.contains(id));
            let count = sel.len();
            let count_text = tn("torrent.library.selected", count);
            let total_text = tn("torrent.library.count", list.len());
            rsx! {
                div { class: "tk-tlib__bar", role: "toolbar", "aria-label": t("torrent.library.actions"),
                    if count > 0 {
                        span { class: "tk-tlib__count", "{count_text}" }
                        Button {
                            label: t("torrent.library.delete_selected"),
                            icon: "trash".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            disabled: busy(),
                            onclick: move |_| {
                                delete_files.set(false);
                                confirm.set(Some(selected().into_iter().collect()));
                            },
                        }
                        Button {
                            label: t("torrent.library.clear_selection"),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| selected.set(BTreeSet::new()),
                        }
                    } else {
                        span { class: "tk-tlib__count", "{total_text}" }
                    }
                }
                div { class: "tk-tlib__wrap",
                    table { class: "tk-tlib__table",
                        thead {
                            tr {
                                th { class: "tk-tlib__col-check", "scope": "col",
                                    CheckBox {
                                        checked: all_selected,
                                        label: t("torrent.library.select_all"),
                                        on_toggle: move |on: bool| {
                                            selected.set(if on { all_ids.clone() } else { BTreeSet::new() });
                                        },
                                    }
                                }
                                SortHeader { label: t("torrent.library.col.name"), sort: LibSort::Name, current: current, on_sort: move |s: (LibSort, bool)| sort.set(s) }
                                SortHeader { label: t("torrent.library.col.episode"), sort: LibSort::Episode, current: current, numeric: true, on_sort: move |s: (LibSort, bool)| sort.set(s) }
                                th { class: "tk-tlib__num", "scope": "col", {t("torrent.library.col.files")} }
                                SortHeader { label: t("torrent.library.col.size"), sort: LibSort::Size, current: current, numeric: true, on_sort: move |s: (LibSort, bool)| sort.set(s) }
                                SortHeader { label: t("torrent.library.col.state"), sort: LibSort::Progress, current: current, on_sort: move |s: (LibSort, bool)| sort.set(s) }
                                SortHeader { label: t("torrent.library.col.added"), sort: LibSort::Added, current: current, on_sort: move |s: (LibSort, bool)| sort.set(s) }
                                th { "scope": "col", {t("torrent.library.col.keep")} }
                                th { class: "tk-tlib__col-actions", "scope": "col",
                                    span { class: "tk-sr-only", {t("torrent.library.actions")} }
                                }
                            }
                        }
                        tbody {
                            for item in rows {
                                LibraryRow {
                                    key: "{item.id}",
                                    selected: sel.contains(&item.id),
                                    item: item.clone(),
                                    busy: busy(),
                                    on_select: move |(id, on): (String, bool)| {
                                        let mut next = selected();
                                        if on {
                                            next.insert(id);
                                        } else {
                                            next.remove(&id);
                                        }
                                        selected.set(next);
                                    },
                                    on_open: move |id: String| open.call(id),
                                    on_folder: move |id: String| open_folder.call(id),
                                    on_keep: move |change: (String, bool)| set_persist.call(change),
                                    on_delete: move |id: String| {
                                        delete_files.set(false);
                                        confirm.set(Some(vec![id]));
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    let dialog = confirm().map(|ids| {
        let title = if ids.len() == 1 {
            t("torrent.library.delete_one")
        } else {
            tf("torrent.library.delete_many", &[("n", &ids.len().to_string())])
        };
        rsx! {
            ConfirmDialog {
                title: title,
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
        section { class: "tk-tlib", "aria-label": t("torrent.section.library"),
            div { class: "tk-tlib__head",
                h2 { class: "tk-tlib__title", {t("torrent.section.library")} }
                p { class: "tk-tlib__intro", {t("torrent.library.intro")} }
                div { class: "tk-tlib__head-actions",
                    Button {
                        label: t("torrent.add.open"),
                        icon: "plus".to_string(),
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            from_os.set(false);
                            adding.set(Some(String::new()));
                        },
                    }
                }
            }
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false, title: Some(t("torrent.library.action_failed")),
                    Button { label: t("common.close"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| action_error.set(None) }
                }
            }
            {body}
        }
        {dialog}
        if let Some(initial) = adding() {
            AddTorrentSheet {
                initial: initial,
                from_os: from_os(),
                on_close: move |_| {
                    adding.set(None);
                    from_os.set(false);
                },
                on_added: move |id: String| {
                    MANUAL_IDS.write().insert(id);
                    adding.set(None);
                    from_os.set(false);
                    refresh.call(());
                },
            }
        }
    }
}

/// "Add torrent": source -> preview -> alignment -> add (module docs).
#[component]
fn AddTorrentSheet(initial: String, from_os: bool, on_close: EventHandler<()>, on_added: EventHandler<String>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut source = use_signal(|| initial.clone());
    let mut media = use_signal(String::new);
    let mut preview = use_signal(|| None::<UiResult<TorrentPreviewVm>>);
    let mut rows = use_signal(Vec::<AlignRow>::new);
    let mut loading = use_signal(|| false);
    let mut adding = use_signal(|| false);
    let mut add_error = use_signal(|| None::<UiError>);
    let mut over = use_signal(|| false);
    // The source the shown preview belongs to (the add uses it, even if the field changed meanwhile).
    let mut previewed = use_signal(String::new);

    let inspect = use_callback({
        let backend = backend.clone();
        move |text: String| {
            let text = text.trim().trim_matches(['"', '\'']).to_owned();
            if text.is_empty() || loading() {
                return;
            }
            loading.set(true);
            add_error.set(None);
            preview.set(None);
            let backend = backend.clone();
            spawn(async move {
                let r = backend.torrent_preview(&text).await;
                if let Ok(p) = &r {
                    rows.set(align_rows(p));
                    previewed.set(text.clone());
                }
                preview.set(Some(r));
                loading.set(false);
            });
        }
    });
    // A link handed over by the OS (or the caller) is inspected right away.
    use_hook(move || {
        if !initial.trim().is_empty() {
            let link = initial.clone();
            spawn(async move { inspect.call(link) });
        }
    });

    let media_text = media();
    let media_ok = parse_media(&media_text);
    let current_rows = rows();
    let alignment = alignment_of(&current_rows);
    let dup = duplicate_episode(&current_rows);

    let add = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Ok(alignment) = alignment_of(&rows.peek()) else { return };
            let Ok(media_id) = parse_media(&media.peek()) else { return };
            if adding() {
                return;
            }
            adding.set(true);
            add_error.set(None);
            let src = previewed.peek().clone();
            let backend = backend.clone();
            spawn(async move {
                match backend.torrent_add_manual(&src, media_id.as_deref(), &alignment).await {
                    Ok(id) => {
                        snack.show(t("torrent.add.added"));
                        on_added.call(id);
                    }
                    Err(e) => add_error.set(Some(e)),
                }
                adding.set(false);
            });
        }
    });

    let content = match &*preview.read() {
        None if loading() => rsx! {
            div { class: "tk-tadd__loading", "aria-busy": "true",
                LoadingIndicator {}
                p { {t("torrent.add.resolving")} }
            }
        },
        None => rsx! {},
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, title: Some(t("torrent.add.preview_failed")),
                on_retry: move |_| inspect.call(source.peek().clone()) }
        },
        Some(Ok(p)) => {
            let pct = format!("{:.0}", (p.confidence.clamp(0.0, 1.0) * 100.0).round());
            let conf_tone = if p.confidence >= 0.8 { Tone::Ok } else if p.confidence >= 0.5 { Tone::Warn } else { Tone::Err };
            let size = format_bytes(p.size_bytes);
            let summary = tf("torrent.add.summary", &[("files", &p.files.len().to_string()), ("size", &size)]);
            rsx! {
                div { class: "tk-tadd__preview",
                    "data-alignment-confidence": "{p.confidence:.2}",
                    "data-info-hash": "{p.info_hash}",
                    div { class: "tk-tadd__name",
                        Icon { name: "magnet".to_string(), size: 20 }
                        span { class: "tk-selectable", title: "{p.name}", "{p.name}" }
                    }
                    p { class: "tk-tadd__summary tk-num", "{summary}" }
                    div { class: "tk-tadd__confidence",
                        Pill { text: tf("torrent.add.confidence", &[("pct", &pct)]), tone: conf_tone }
                        span { class: "tk-tadd__hint", {t("torrent.add.confidence_hint")} }
                    }
                    TextField {
                        label: t("torrent.add.media"),
                        value: media_text.clone(),
                        icon: "film".to_string(),
                        mono: true,
                        error: media_ok.is_err(),
                        hint: if media_ok.is_err() { t("torrent.add.media_invalid") } else { t("torrent.add.media_hint") },
                        oninput: move |v: String| media.set(v),
                    }
                    if current_rows.is_empty() {
                        EmptyState { icon: "folder".to_string(), title: t("torrent.add.no_files") }
                    } else {
                        div { class: "tk-ttable-wrap",
                            table { class: "tk-ttable tk-tadd__table",
                                thead {
                                    tr {
                                        th { "scope": "col", span { class: "tk-sr-only", {t("torrent.add.col.include")} } }
                                        th { "scope": "col", {t("torrent.add.col.file")} }
                                        th { "scope": "col", "data-numeric": "true", {t("torrent.add.col.size")} }
                                        th { "scope": "col", {t("torrent.add.col.episode")} }
                                    }
                                }
                                tbody {
                                    for (pos , r) in current_rows.iter().cloned().enumerate() {
                                        tr {
                                            key: "{r.index}",
                                            "data-file-index": "{r.index}",
                                            "data-file-episode": "{r.episode.trim()}",
                                            "data-included": "{r.include}",
                                            td {
                                                button {
                                                    class: "tk-tlib__check",
                                                    r#type: "button",
                                                    role: "checkbox",
                                                    "aria-checked": "{r.include}",
                                                    "aria-label": tf("torrent.add.include", &[("name", &r.name)]),
                                                    "data-checked": "{r.include}",
                                                    onclick: move |_| rows.with_mut(|v| {
                                                        if let Some(x) = v.get_mut(pos) {
                                                            x.include = !x.include;
                                                        }
                                                    }),
                                                    if r.include {
                                                        Icon { name: "check".to_string(), size: 16 }
                                                    }
                                                }
                                            }
                                            td { class: "tk-ttable__main",
                                                span { class: "tk-tfiles__name tk-selectable", title: "{r.path}", "{r.name}" }
                                                if !r.video {
                                                    span { class: "tk-tfiles__tag", Pill { text: t("torrent.add.not_video"), plain: true } }
                                                }
                                            }
                                            td { class: "tk-num", "data-numeric": "true", {format_bytes(r.size)} }
                                            td {
                                                input {
                                                    class: "tk-tadd__episode",
                                                    r#type: "text",
                                                    inputmode: "numeric",
                                                    name: "episode-{r.index}",
                                                    "aria-label": tf("torrent.add.episode_for", &[("name", &r.name)]),
                                                    placeholder: "-",
                                                    initial_value: "{r.episode}",
                                                    disabled: !r.include,
                                                    oninput: move |e| {
                                                        let v = e.value();
                                                        rows.with_mut(|list| {
                                                            if let Some(x) = list.get_mut(pos) {
                                                                x.episode = v;
                                                            }
                                                        });
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    {match (&alignment, dup) {
                        (Err(name), _) if name.is_empty() => rsx! { p { class: "tk-tadd__warn", role: "alert", {t("torrent.add.none_selected")} } },
                        (Err(name), _) => rsx! { p { class: "tk-tadd__warn", role: "alert", {tf("torrent.add.bad_episode", &[("name", name)])} } },
                        (Ok(_), Some(ep)) => rsx! { p { class: "tk-tadd__warn", {tf("torrent.add.duplicate", &[("episode", &ep.to_string())])} } },
                        _ => rsx! {},
                    }}
                    if let Some(e) = add_error() {
                        ErrorState { error: e, compact: true, title: Some(t("torrent.add.add_failed")), on_retry: move |_| add.call(()) }
                    }
                }
            }
        }
    };

    let can_add = matches!(&*preview.read(), Some(Ok(_))) && alignment.is_ok() && media_ok.is_ok() && !adding();
    let footer = rsx! {
        Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_close.call(()) }
        Button {
            label: if adding() { t("torrent.add.adding") } else { t("torrent.add.add") },
            icon: "plus".to_string(),
            disabled: !can_add,
            onclick: move |_| add.call(()),
        }
    };
    let drop_label = t("torrent.add.choose_file");

    rsx! {
        Sheet { title: t("torrent.add.title"), wide: true, on_close: move |_| on_close.call(()), footer: footer,
            div { class: "tk-tadd", "data-pending-magnet": "{from_os}",
                if from_os {
                    p { class: "tk-tadd__os", Icon { name: "magnet".to_string(), size: 18 } {t("torrent.add.from_os")} }
                }
                div { class: "tk-tadd__source",
                    TextField {
                        label: t("torrent.add.source"),
                        value: source(),
                        icon: "magnet".to_string(),
                        mono: true,
                        placeholder: "magnet:?xt=urn:btih:…".to_string(),
                        hint: t("torrent.add.source_hint"),
                        oninput: move |v: String| source.set(v),
                        onsubmit: move |_| inspect.call(source.peek().clone()),
                    }
                    Button {
                        label: t("torrent.add.inspect"),
                        icon: "search".to_string(),
                        variant: ButtonVariant::Tonal,
                        disabled: loading() || source().trim().is_empty(),
                        onclick: move |_| inspect.call(source.peek().clone()),
                    }
                }
                label {
                    class: "tk-drop tk-tadd__drop",
                    r#for: "{TORRENT_INPUT_ID}",
                    role: "button",
                    tabindex: "0",
                    "aria-label": "{drop_label}",
                    "data-over": "{over()}",
                    ondragenter: move |e| {
                        e.prevent_default();
                        over.set(true);
                    },
                    ondragover: move |e| {
                        e.prevent_default();
                        if !*over.peek() {
                            over.set(true);
                        }
                    },
                    ondragleave: move |_| over.set(false),
                    ondrop: move |e| {
                        e.prevent_default();
                        over.set(false);
                        if let Some(f) = e.data_transfer().files().first() {
                            let path = f.path().to_string_lossy().into_owned();
                            source.set(path.clone());
                            inspect.call(path);
                        }
                    },
                    onkeydown: move |e| {
                        let activate = match e.key() {
                            Key::Enter => true,
                            Key::Character(c) => c == " ",
                            _ => false,
                        };
                        if activate {
                            e.prevent_default();
                            let _ = document::eval(&format!("const i=document.getElementById('{TORRENT_INPUT_ID}');if(i){{i.click();}}"));
                        }
                    },
                    Icon { name: "upload".to_string(), size: 24 }
                    b { "{drop_label}" }
                    span { class: "tk-drop__hint", {t("torrent.add.drop_hint")} }
                    input {
                        id: "{TORRENT_INPUT_ID}",
                        r#type: "file",
                        accept: ".torrent",
                        hidden: true,
                        onchange: move |e| {
                            if let Some(f) = e.files().first() {
                                let path = f.path().to_string_lossy().into_owned();
                                source.set(path.clone());
                                inspect.call(path);
                            }
                        },
                    }
                }
                {content}
            }
        }
    }
}

/// A sortable column header: a click sorts by it; a second click flips the direction.
#[component]
fn SortHeader(
    label: String,
    sort: LibSort,
    current: (LibSort, bool),
    #[props(default)] numeric: bool,
    on_sort: EventHandler<(LibSort, bool)>,
) -> Element {
    let (by, desc) = current;
    let active = by == sort;
    let aria = match (active, desc) {
        (false, _) => "none",
        (true, true) => "descending",
        (true, false) => "ascending",
    };
    let icon = if active && !desc { "chevron-up" } else { "chevron-down" };
    let class = if numeric { "tk-tlib__num" } else { "" };
    rsx! {
        th { class: "{class}", "scope": "col", "aria-sort": aria,
            button {
                class: "tk-tlib__sort",
                r#type: "button",
                "data-active": "{active}",
                // A new column starts descending for numbers / dates, ascending for text.
                onclick: move |_| on_sort.call(if active { (sort, !desc) } else { (sort, sort != LibSort::Name) }),
                span { "{label}" }
                Icon { name: icon.to_string(), size: 16 }
            }
        }
    }
}

/// Square check control (`role="checkbox"`), so selection looks the same in WebKitGTK and WebView2.
#[component]
fn CheckBox(checked: bool, label: String, #[props(default)] on_toggle: EventHandler<bool>) -> Element {
    rsx! {
        button {
            class: "tk-tlib__check",
            r#type: "button",
            role: "checkbox",
            "aria-checked": "{checked}",
            "aria-label": "{label}",
            title: "{label}",
            "data-checked": "{checked}",
            onclick: move |_| on_toggle.call(!checked),
            if checked {
                Icon { name: "check".to_string(), size: 16 }
            }
        }
    }
}

#[component]
fn LibraryRow(
    item: TorrentLibraryItemVm,
    selected: bool,
    busy: bool,
    on_select: EventHandler<(String, bool)>,
    on_open: EventHandler<String>,
    on_folder: EventHandler<String>,
    on_keep: EventHandler<(String, bool)>,
    on_delete: EventHandler<String>,
) -> Element {
    let title = display_title(&item);
    let show_name = item.series.as_deref().is_some_and(|s| !s.trim().is_empty());
    let episode = item.episode.map(|e| e.to_string()).unwrap_or_else(|| "-".to_owned());
    let size = format_bytes(item.size_bytes);
    let pct = format_percent(item.progress);
    let state = t(&format!("torrent.state.{}", item.state.id()));
    let added = format_date(item.added_unix);
    let added_full = if item.added_unix > 0 { format_unix_utc(item.added_unix) } else { String::new() };
    let downloading = item.state == TorrentStateVm::Downloading;
    let select_label = tf("torrent.library.select", &[("name", &title)]);
    let progress_label = tf("torrent.library.progress", &[("pct", &pct)]);
    let poster = item.poster.clone().filter(|p| !p.is_empty());
    let (id1, id2, id3, id4, id5) = (item.id.clone(), item.id.clone(), item.id.clone(), item.id.clone(), item.id.clone());
    let manual = item.media_id.is_none() || MANUAL_IDS.read().contains(&item.id);
    rsx! {
        tr { class: "tk-tlib__row", "data-selected": "{selected}", "data-state": item.state.id(), "data-manual": "{manual}",
            td { class: "tk-tlib__col-check",
                CheckBox { checked: selected, label: select_label, on_toggle: move |on: bool| on_select.call((id1.clone(), on)) }
            }
            td { class: "tk-tlib__name", "data-label": t("torrent.library.col.name"),
                div { class: "tk-tlib__series",
                    if let Some(src) = poster {
                        img { class: "tk-tlib__poster", src: "{src}", alt: "", "loading": "lazy" }
                    } else {
                        span { class: "tk-tlib__poster tk-tlib__poster--empty", "aria-hidden": "true",
                            Icon { name: "magnet".to_string(), size: 18 }
                        }
                    }
                    div { class: "tk-tlib__titles",
                        span { class: "tk-tlib__series-title", title: "{title}", "{title}" }
                        if show_name {
                            span { class: "tk-tlib__file", title: "{item.name}", "{item.name}" }
                        }
                        span { class: "tk-tlib__path", title: "{item.path}", "{item.path}" }
                    }
                }
            }
            td { class: "tk-tlib__num", "data-label": t("torrent.library.col.episode"), "{episode}" }
            td { class: "tk-tlib__num", "data-label": t("torrent.library.col.files"), "{item.files}" }
            td { class: "tk-tlib__num", "data-label": t("torrent.library.col.size"), "{size}" }
            td { class: "tk-tlib__state", "data-label": t("torrent.library.col.state"),
                div { class: "tk-tlib__state-line",
                    Pill { text: state, tone: state_tone(item.state) }
                    span { class: "tk-tlib__pct", "{pct}" }
                }
                WavyProgress { fraction: item.progress, label: progress_label, active: downloading, thin: true }
            }
            td { class: "tk-tlib__date", "data-label": t("torrent.library.col.added"), title: "{added_full}", "{added}" }
            td { class: "tk-tlib__keep", "data-label": t("torrent.library.col.keep"),
                Switch {
                    checked: item.persist,
                    label: t("torrent.library.keep"),
                    disabled: busy,
                    on_toggle: move |v: bool| on_keep.call((id2.clone(), v)),
                }
            }
            td { class: "tk-tlib__col-actions",
                div { class: "tk-tlib__actions",
                    IconButton {
                        icon: "external".to_string(),
                        label: t("torrent.library.open"),
                        size: IconButtonSize::Sm,
                        onclick: move |_| on_open.call(id3.clone()),
                    }
                    IconButton {
                        icon: "folder".to_string(),
                        label: t("torrent.library.open_folder"),
                        size: IconButtonSize::Sm,
                        onclick: move |_| on_folder.call(id4.clone()),
                    }
                    IconButton {
                        icon: "trash".to_string(),
                        label: t("torrent.library.delete"),
                        size: IconButtonSize::Sm,
                        disabled: busy,
                        onclick: move |_| on_delete.call(id5.clone()),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, series: Option<&str>, size: u64, added: i64, episode: Option<u32>) -> TorrentLibraryItemVm {
        TorrentLibraryItemVm {
            id: id.to_owned(),
            name: format!("[Group] {id}.mkv"),
            media_id: None,
            series: series.map(str::to_owned),
            episode,
            poster: None,
            files: 1,
            size_bytes: size,
            progress: 0.5,
            state: TorrentStateVm::Downloading,
            added_unix: added,
            persist: true,
            path: "/tmp".to_owned(),
        }
    }

    #[test]
    fn byte_speed_and_time_formats() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_380_000_000), "1.38 GB");
        assert_eq!(format_bytes(45_600_000), "45.6 MB");
        assert_eq!(format_bytes(512_000_000), "512 MB");
        assert_eq!(format_speed(4_200_000), "4.20 MB/s");
        assert_eq!(format_percent(0.42), "42%");
        assert_eq!(format_percent(0.9995), "99.9%");
        assert_eq!(format_percent(1.0), "100%");
        assert_eq!(format_percent(7.0), "100%");
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(185), "3m 05s");
        assert_eq!(format_duration(3720), "1h 02m");
        assert_eq!(format_date(1_700_000_000), "2023-11-14");
        assert_eq!(format_date(0), "");
    }

    #[test]
    fn sorting_is_stable_and_reversible() {
        let items = vec![
            item("a", Some("beta"), 30, 3, Some(2)),
            item("b", Some("Alpha"), 10, 1, None),
            item("c", None, 20, 2, Some(1)),
        ];
        let ids = |v: Vec<TorrentLibraryItemVm>| v.into_iter().map(|i| i.id).collect::<Vec<_>>();
        assert_eq!(ids(sort_items(&items, LibSort::Added, true)), ["a", "c", "b"]);
        assert_eq!(ids(sort_items(&items, LibSort::Size, false)), ["b", "c", "a"]);
        // "[Group] c.mkv" (no series) sorts by its torrent name, before the letters
        assert_eq!(ids(sort_items(&items, LibSort::Name, false)), ["c", "b", "a"]);
        assert_eq!(ids(sort_items(&items, LibSort::Episode, false)), ["c", "a", "b"]);
    }

    fn preview() -> TorrentPreviewVm {
        let file = |index: usize, path: &str| crate::backend::TorrentFileVm {
            index,
            path: path.to_owned(),
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            size_bytes: 10,
            downloaded_bytes: 0,
            progress: 0.0,
            selected: true,
            streams: 0,
            episode: None,
        };
        TorrentPreviewVm {
            name: "Show".into(),
            info_hash: "a".repeat(40),
            size_bytes: 30,
            files: vec![file(0, "Show/readme.txt"), file(1, "Show/Show - 02.mkv"), file(2, "Show/Show - 01.mkv")],
            suggested: vec![(2, 1), (1, 2)],
            confidence: 1.0,
        }
    }

    #[test]
    fn alignment_rows_from_a_preview() {
        let rows = align_rows(&preview());
        // videos first, by path; the text file is listed but not included
        assert_eq!(rows.iter().map(|r| r.index).collect::<Vec<_>>(), [2, 1, 0]);
        assert_eq!((rows[0].episode.as_str(), rows[1].episode.as_str()), ("1", "2"));
        assert!(!rows[2].include && rows[2].episode.is_empty());
        assert_eq!(alignment_of(&rows), Ok(vec![(2, 1), (1, 2)]));
        let mut edited = rows.clone();
        edited[1].episode = "x".into();
        assert_eq!(alignment_of(&edited), Err("Show - 02.mkv".to_owned()));
        edited[1].episode = String::new();
        assert_eq!(alignment_of(&edited), Ok(vec![(2, 1), (1, 0)]));
        edited[1].episode = "1".into();
        assert_eq!(duplicate_episode(&edited), Some(1));
        let none: Vec<AlignRow> = rows.into_iter().map(|r| AlignRow { include: false, ..r }).collect();
        assert_eq!(alignment_of(&none), Err(String::new()));
    }

    #[test]
    fn media_ids() {
        assert_eq!(parse_media(""), Ok(None));
        assert_eq!(parse_media(" 21 "), Ok(Some("21".to_owned())));
        assert_eq!(parse_media("one piece"), Err(()));
    }

    #[test]
    fn selection_drops_removed_rows() {
        let items = vec![item("a", None, 1, 1, None)];
        let sel: BTreeSet<String> = ["a".to_owned(), "gone".to_owned()].into_iter().collect();
        assert_eq!(prune_selection(&sel, &items).into_iter().collect::<Vec<_>>(), ["a"]);
    }
}
