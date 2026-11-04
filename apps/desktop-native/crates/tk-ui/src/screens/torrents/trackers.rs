//! Torrent client > Trackers: sortable table url / status (`torrent.tracker.*` pill) / downloaded / seeders /
//! leechers / message (`torrent_trackers`, polled every 3 s). Counts the tracker did not report show a dash.
//! Compact windows get a list of cards (CSS). Owner: wave 3 C.

use dioxus::prelude::*;

use super::layout::common::{POLL_SLOW, SortBy, SortTh, TorrentPicker, format_opt, tracker_tone, use_torrent_poll};
use crate::app::Ui;
use crate::backend::{TorrentTrackerVm, TrackerStatusVm};
use crate::components::{EmptyState, ErrorState, Pill, SkeletonLines};
use crate::i18n::{t, tf};
use crate::model::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Url,
    Status,
    Downloaded,
    Seeders,
    Leechers,
}

/// (column, header key, numeric). Numeric columns sort high-to-low on the first click.
const COLUMNS: [(Col, &str, bool); 5] = [
    (Col::Url, "torrent.trackers.col.url", false),
    (Col::Status, "torrent.trackers.col.status", false),
    (Col::Downloaded, "torrent.trackers.col.downloaded", true),
    (Col::Seeders, "torrent.trackers.col.seeders", true),
    (Col::Leechers, "torrent.trackers.col.leechers", true),
];

/// Working first, then updating, failed, not contacted.
fn status_rank(s: TrackerStatusVm) -> u8 {
    match s {
        TrackerStatusVm::Working => 0,
        TrackerStatusVm::Updating => 1,
        TrackerStatusVm::Failed => 2,
        TrackerStatusVm::Unknown => 3,
    }
}

fn sort_trackers(list: &mut [TorrentTrackerVm], sort: SortBy<Col>) {
    list.sort_by(|a, b| {
        let ord = match sort.col {
            Col::Url => a.url.cmp(&b.url),
            Col::Status => status_rank(a.status).cmp(&status_rank(b.status)),
            Col::Downloaded => a.downloaded.cmp(&b.downloaded),
            Col::Seeders => a.seeders.cmp(&b.seeders),
            Col::Leechers => a.leechers.cmp(&b.leechers),
        };
        sort.apply(ord).then_with(|| a.url.cmp(&b.url))
    });
}

#[component]
pub fn TorrentTrackersPage() -> Element {
    let ui = use_context::<Ui>();
    let trackers = use_torrent_poll(POLL_SLOW, |b, id| async move { b.torrent_trackers(&id).await });
    let mut sort = use_signal(|| SortBy::new(Col::Status, false));

    let selected = ui.torrent.read().clone();
    let Some(id) = selected else {
        return rsx! {
            TorrentPicker {}
        };
    };

    let cur = sort();
    let content = match trackers.for_id(&id) {
        None => rsx! {
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![90, 80, 85] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState {
                error: e.clone(),
                title: Some(t("torrent.trackers.error")),
                retry: e.kind != ErrorKind::NotWired,
                on_retry: move |_| trackers.refresh(),
            }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "cast".to_string(), title: t("torrent.trackers.empty_title"), body: t("torrent.trackers.empty_body") }
        },
        Some(Ok(mut list)) => {
            let working = list.iter().filter(|x| x.status == TrackerStatusVm::Working).count();
            let summary =
                tf("torrent.trackers.summary", &[("n", &list.len().to_string()), ("working", &working.to_string())]);
            sort_trackers(&mut list, cur);
            let [l_url, l_status, l_downloaded, l_seeders, l_leechers] = COLUMNS.map(|(_, k, _)| t(k));
            let l_message = t("torrent.trackers.col.message");
            rsx! {
                p { class: "tk-ttable__summary tk-num", "{summary}" }
                div { class: "tk-ttable-wrap",
                    table { class: "tk-ttable tk-ttable--trackers",
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
                                th { class: "tk-ttable__th", "scope": "col", "{l_message}" }
                            }
                        }
                        tbody {
                            for tr_vm in list {
                                {
                                    let status = t(&format!("torrent.tracker.{}", tr_vm.status.id()));
                                    let tone = tracker_tone(tr_vm.status);
                                    let message = tr_vm.message.clone().unwrap_or_default();
                                    rsx! {
                                        tr { key: "{tr_vm.url}", "data-status": "{tr_vm.status.id()}",
                                            td { class: "tk-ttable__main tk-mono tk-selectable tk-ttrackers__url", "data-label": "{l_url}", title: "{tr_vm.url}", "{tr_vm.url}" }
                                            td { "data-label": "{l_status}",
                                                Pill { text: status, tone: tone }
                                            }
                                            td { "data-numeric": "true", "data-label": "{l_downloaded}", class: "tk-num", {format_opt(tr_vm.downloaded)} }
                                            td { "data-numeric": "true", "data-label": "{l_seeders}", class: "tk-num", {format_opt(tr_vm.seeders)} }
                                            td { "data-numeric": "true", "data-label": "{l_leechers}", class: "tk-num", {format_opt(tr_vm.leechers)} }
                                            td { "data-label": "{l_message}", class: "tk-ttrackers__msg tk-selectable", "{message}" }
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
        section { class: "tk-tpage tk-ttrackers",
            {content}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(url: &str, status: TrackerStatusVm, seeders: Option<u32>) -> TorrentTrackerVm {
        TorrentTrackerVm { url: url.to_owned(), status, message: None, downloaded: None, seeders, leechers: None }
    }

    #[test]
    fn working_trackers_first_unknown_counts_last() {
        let mut list = vec![
            tracker("udp://b", TrackerStatusVm::Failed, None),
            tracker("udp://a", TrackerStatusVm::Unknown, None),
            tracker("udp://c", TrackerStatusVm::Working, Some(40)),
        ];
        sort_trackers(&mut list, SortBy::new(Col::Status, false));
        assert_eq!(list.iter().map(|x| x.url.as_str()).collect::<Vec<_>>(), ["udp://c", "udp://b", "udp://a"]);
        sort_trackers(&mut list, SortBy::new(Col::Seeders, true));
        assert_eq!(list[0].url, "udp://c");
    }
}
