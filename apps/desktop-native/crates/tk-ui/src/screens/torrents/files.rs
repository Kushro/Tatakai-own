//! Torrent client > Files: searchable, sortable table name / size / progress / streams / episode
//! (`torrent_files`, polled every second). Files outside the selection are dimmed and tagged "Skipped"; files
//! being streamed show a live badge. In compact windows the table turns into a list of cards (CSS). Owner:
//! wave 3 C.
//!
//! Wave A (worker i, checklist `torrent.files.play-file`): every video file has "Play" (torrents added for a title:
//! the file is aligned to its episode with `torrent_add_manual(<info-hash>, media, [(index, episode)])`, picked for
//! that episode as `manual|torrent|<info-hash>` and the player opens it) and "Download only this"
//! (`torrent_add_manual` with the single pair `(index, 0)`: the selection becomes that file).

use dioxus::prelude::*;

use super::layout::common::{
    CellProgress, POLL_FAST, SortBy, SortTh, TorrentPicker, cmp_f64, format_bytes, format_opt, matches_query,
    use_torrent_list, use_torrent_poll,
};
use crate::app::{Route, Services, Ui};
use crate::backend::TorrentFileVm;
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize, Pill, SkeletonLines, TextField,
    use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{ErrorKind, UiError};

/// Source id of a torrent added by hand (same form as tk-app's `torrent_play::manual_source_id`).
pub(super) fn manual_source_id(info_hash: &str) -> String {
    format!("manual|torrent|{}", info_hash.to_ascii_lowercase())
}

/// Whether the player can open this file (a video).
fn is_video(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "mkv" | "mp4" | "avi" | "webm" | "m2ts" | "ts" | "mov" | "wmv" | "flv")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Name,
    Size,
    Progress,
    Streams,
    Episode,
}

/// (column, header key, numeric). Numeric columns sort high-to-low on the first click.
const COLUMNS: [(Col, &str, bool); 5] = [
    (Col::Name, "torrent.files.col.name", false),
    (Col::Size, "torrent.files.col.size", true),
    (Col::Progress, "torrent.files.col.progress", true),
    (Col::Streams, "torrent.files.col.streams", true),
    (Col::Episode, "torrent.files.col.episode", true),
];

/// Files whose path matches every word of `query`, in `sort` order (ties by torrent index, stable).
fn visible_files(files: &[TorrentFileVm], query: &str, sort: SortBy<Col>) -> Vec<TorrentFileVm> {
    let mut rows: Vec<TorrentFileVm> =
        files.iter().filter(|f| matches_query(query, &[f.path.as_str(), f.name.as_str()])).cloned().collect();
    rows.sort_by(|a, b| {
        let ord = match sort.col {
            Col::Name => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
            Col::Size => a.size_bytes.cmp(&b.size_bytes),
            Col::Progress => cmp_f64(a.progress, b.progress),
            Col::Streams => a.streams.cmp(&b.streams),
            Col::Episode => a.episode.cmp(&b.episode),
        };
        sort.apply(ord).then_with(|| a.index.cmp(&b.index))
    });
    rows
}

/// Folder part of a `/`-separated path inside the torrent (`""` at the root).
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("")
}

#[component]
pub fn TorrentFilesPage() -> Element {
    let ui = use_context::<Ui>();
    let files = use_torrent_poll(POLL_FAST, |b, id| async move { b.torrent_files(&id).await });
    let mut query = use_signal(String::new);
    let mut sort = use_signal(|| SortBy::new(Col::Name, false));
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let list = use_torrent_list();
    let mut busy = use_signal(|| false);
    let mut action_error = use_signal(|| None::<UiError>);

    // (torrent id, media id, file index, episode or 0 for "download only this")
    let act = use_callback({
        let backend = services.backend.clone();
        move |(tid, media, index, episode): (String, Option<String>, usize, u32)| {
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                let result = backend.torrent_add_manual(&tid, media.as_deref(), &[(index, episode)]).await;
                match (result, media) {
                    (Ok(_), Some(media)) if episode > 0 => {
                        let source = manual_source_id(&tid);
                        if let Err(e) = backend.set_picked_source(&media, episode, Some(&source)).await {
                            tracing::warn!(detail = %e.detail, "picking the manual torrent failed");
                        }
                        navigator.push(Route::PlayerPage { id: media, episode });
                    }
                    (Ok(_), _) => {
                        snack.show(t("torrent.files.only_this_started"));
                        files.refresh();
                    }
                    (Err(e), _) => action_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let selected = ui.torrent.read().clone();
    let Some(id) = selected else {
        return rsx! {
            TorrentPicker {}
        };
    };

    let media_id: Option<String> = match &*list.read() {
        Some(Ok(items)) => items.iter().find(|i| i.id == id).and_then(|i| i.media_id.clone()),
        _ => None,
    };
    let q = query();
    let cur = sort();
    let content = match files.for_id(&id) {
        None => rsx! {
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![90, 75, 85, 60, 70] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState {
                error: e.clone(),
                title: Some(t("torrent.files.error")),
                retry: e.kind != ErrorKind::NotWired,
                on_retry: move |_| files.refresh(),
            }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "folder".to_string(), title: t("torrent.files.empty_title"), body: t("torrent.files.empty_body") }
        },
        Some(Ok(list)) => {
            let rows = visible_files(&list, &q, cur);
            let picked: Vec<&TorrentFileVm> = list.iter().filter(|f| f.selected).collect();
            let summary = tf(
                "torrent.files.summary",
                &[
                    ("files", &tn("torrent.files.count", list.len())),
                    ("selected", &tn("torrent.files.selected_n", picked.len())),
                    ("size", &format_bytes(picked.iter().map(|f| f.size_bytes).sum())),
                ],
            );
            let (l_name, l_size, l_progress, l_streams, l_episode) = (
                t("torrent.files.col.name"),
                t("torrent.files.col.size"),
                t("torrent.files.col.progress"),
                t("torrent.files.col.streams"),
                t("torrent.files.col.episode"),
            );
            let (skipped, live) = (t("torrent.files.skipped"), t("torrent.files.streaming"));
            rsx! {
                p { class: "tk-ttable__summary tk-num", "{summary}" }
                if rows.is_empty() {
                    EmptyState {
                        icon: "search".to_string(),
                        title: t("torrent.files.no_match"),
                        body: tf("torrent.files.no_match_body", &[("query", q.trim())]),
                        action_label: t("torrent.files.clear_search"),
                        on_action: move |_| query.set(String::new()),
                    }
                } else {
                    div { class: "tk-ttable-wrap",
                        table { class: "tk-ttable tk-ttable--files",
                            thead {
                                tr {
                                    for (col , label_key , numeric) in COLUMNS {
                                        SortTh {
                                            key: "{label_key}",
                                            label: t(label_key),
                                            sort: cur.aria(col),
                                            numeric: numeric,
                                            on_sort: move |_| {
                                                let next = sort.peek().click(col, numeric);
                                                sort.set(next);
                                            },
                                        }
                                    }
                                    th { class: "tk-ttable__th", "scope": "col",
                                        span { class: "tk-sr-only", {t("torrent.files.col.actions")} }
                                    }
                                }
                            }
                            tbody {
                                for f in rows {
                                    {
                                        let folder = folder_of(&f.path).to_owned();
                                        let size = format_bytes(f.size_bytes);
                                        let episode = format_opt(f.episode);
                                        let video = is_video(&f.name);
                                        let play_ep = f.episode.unwrap_or(1);
                                        let (tid1, tid2) = (id.clone(), id.clone());
                                        let (m1, m2) = (media_id.clone(), media_id.clone());
                                        let index = f.index;
                                        let play_label = tf("torrent.files.play", &[("name", &f.name)]);
                                        let only_label = tf("torrent.files.only_this", &[("name", &f.name)]);
                                        rsx! {
                                            tr { key: "{f.index}", "data-selected": "{f.selected}",
                                                td { class: "tk-ttable__main", "data-label": "{l_name}",
                                                    span { class: "tk-tfiles__name tk-selectable", title: "{f.path}", "{f.name}" }
                                                    if !folder.is_empty() {
                                                        span { class: "tk-tfiles__dir", "{folder}" }
                                                    }
                                                    if !f.selected {
                                                        span { class: "tk-tfiles__tag",
                                                            Pill { text: skipped.clone(), plain: true }
                                                        }
                                                    }
                                                }
                                                td { "data-numeric": "true", "data-label": "{l_size}", class: "tk-num", "{size}" }
                                                td { "data-label": "{l_progress}",
                                                    CellProgress { fraction: f.progress, label: f.name.clone() }
                                                }
                                                td { "data-numeric": "true", "data-label": "{l_streams}", class: "tk-num",
                                                    if f.streams > 0 {
                                                        span { class: "tk-tfiles__live", title: "{live}",
                                                            Icon { name: "play".to_string(), size: 16 }
                                                            "{f.streams}"
                                                        }
                                                    } else {
                                                        "0"
                                                    }
                                                }
                                                td { "data-numeric": "true", "data-label": "{l_episode}", class: "tk-num", "{episode}" }
                                                td { class: "tk-tfiles__actions",
                                                    if video {
                                                        if m1.is_some() {
                                                            IconButton {
                                                                icon: "play".to_string(),
                                                                label: play_label,
                                                                size: IconButtonSize::Sm,
                                                                disabled: busy(),
                                                                onclick: move |_| act.call((tid1.clone(), m1.clone(), index, play_ep)),
                                                            }
                                                        }
                                                        IconButton {
                                                            icon: "download".to_string(),
                                                            label: only_label,
                                                            size: IconButtonSize::Sm,
                                                            disabled: busy(),
                                                            onclick: move |_| act.call((tid2.clone(), m2.clone(), index, 0)),
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
        }
    };

    rsx! {
        section { class: "tk-tpage tk-tfiles",
            div { class: "tk-ttable__toolbar",
                TextField {
                    label: t("torrent.files.search"),
                    value: q.clone(),
                    kind: "search".to_string(),
                    icon: "search".to_string(),
                    placeholder: t("torrent.files.search_placeholder"),
                    oninput: move |v: String| query.set(v),
                }
            }
            if media_id.is_none() {
                p { class: "tk-tfiles__note", {t("torrent.files.play_needs_title")} }
            }
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false, title: Some(t("torrent.files.action_failed")),
                    Button { label: t("common.close"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| action_error.set(None) }
                }
            }
            {content}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: usize, path: &str, size: u64, progress: f64, episode: Option<u32>) -> TorrentFileVm {
        TorrentFileVm {
            index,
            path: path.to_owned(),
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            size_bytes: size,
            downloaded_bytes: 0,
            progress,
            selected: true,
            streams: 0,
            episode,
        }
    }

    #[test]
    fn search_and_sort_keep_ties_stable() {
        let files = vec![
            file(0, "Show S01/Show - 02.mkv", 300, 0.5, Some(2)),
            file(1, "Show S01/Show - 01.mkv", 300, 1.0, Some(1)),
            file(2, "Show S01/Extras/NCOP.mkv", 50, 0.0, None),
        ];
        let by_name = visible_files(&files, "", SortBy::new(Col::Name, false));
        assert_eq!(by_name.iter().map(|f| f.index).collect::<Vec<_>>(), [2, 1, 0]);
        let by_size = visible_files(&files, "", SortBy::new(Col::Size, true));
        assert_eq!(by_size.iter().map(|f| f.index).collect::<Vec<_>>(), [0, 1, 2]);
        let by_ep = visible_files(&files, "", SortBy::new(Col::Episode, false));
        assert_eq!(by_ep.iter().map(|f| f.index).collect::<Vec<_>>(), [2, 1, 0]);
        let found = visible_files(&files, "show 01.mkv", SortBy::new(Col::Name, false));
        assert_eq!(found.iter().map(|f| f.index).collect::<Vec<_>>(), [1]);
        assert!(visible_files(&files, "ncop extras", SortBy::new(Col::Name, false)).len() == 1);
    }

    #[test]
    fn play_helpers() {
        assert_eq!(manual_source_id("ABC"), "manual|torrent|abc");
        assert!(is_video("Show - 01.MKV") && !is_video("notes.txt"));
    }

    #[test]
    fn folder_of_path() {
        assert_eq!(folder_of("a/b/c.mkv"), "a/b");
        assert_eq!(folder_of("c.mkv"), "");
    }
}
