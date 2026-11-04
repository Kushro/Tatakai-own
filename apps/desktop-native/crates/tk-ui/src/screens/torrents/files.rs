//! Torrent client > Files: searchable, sortable table name / size / progress / streams / episode
//! (`torrent_files`, polled every second). Files outside the selection are dimmed and tagged "Skipped"; files
//! being streamed show a live badge. In compact windows the table turns into a list of cards (CSS). Owner:
//! wave 3 C.

use dioxus::prelude::*;

use super::layout::common::{
    CellProgress, POLL_FAST, SortBy, SortTh, TorrentPicker, cmp_f64, format_bytes, format_opt, matches_query,
    use_torrent_poll,
};
use crate::app::Ui;
use crate::backend::TorrentFileVm;
use crate::components::{EmptyState, ErrorState, Pill, SkeletonLines, TextField};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::ErrorKind;

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

    let selected = ui.torrent.read().clone();
    let Some(id) = selected else {
        return rsx! {
            TorrentPicker {}
        };
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
                    ("n", &list.len().to_string()),
                    ("selected", &picked.len().to_string()),
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
                                }
                            }
                            tbody {
                                for f in rows {
                                    {
                                        let folder = folder_of(&f.path).to_owned();
                                        let size = format_bytes(f.size_bytes);
                                        let episode = format_opt(f.episode);
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
    fn folder_of_path() {
        assert_eq!(folder_of("a/b/c.mkv"), "a/b");
        assert_eq!(folder_of("c.mkv"), "");
    }
}
