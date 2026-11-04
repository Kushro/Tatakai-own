//! Torrent client > Peers: sortable table ip:port / client / progress / down / up / downloaded / uploaded /
//! country (ISO code, no flag images) / flags with a legend (`torrent.peers.flag.<letter>`). `torrent_peers` is
//! polled every second; fastest first by default. Compact windows get a list of cards (CSS). Owner: wave 3 C.

use dioxus::prelude::*;

use super::layout::common::{
    CellProgress, POLL_FAST, SortBy, SortTh, TorrentPicker, cmp_f64, format_bytes, format_speed, use_torrent_poll,
};
use crate::app::Ui;
use crate::backend::TorrentPeerVm;
use crate::components::{EmptyState, ErrorState, SkeletonLines};
use crate::i18n::{t, tf};
use crate::model::ErrorKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Col {
    Addr,
    Client,
    Progress,
    Down,
    Up,
    Downloaded,
    Uploaded,
    Country,
    Flags,
}

/// (column, header key, numeric). Numeric columns sort high-to-low on the first click.
const COLUMNS: [(Col, &str, bool); 9] = [
    (Col::Addr, "torrent.peers.col.addr", false),
    (Col::Client, "torrent.peers.col.client", false),
    (Col::Progress, "torrent.peers.col.progress", true),
    (Col::Down, "torrent.peers.col.down", true),
    (Col::Up, "torrent.peers.col.up", true),
    (Col::Downloaded, "torrent.peers.col.downloaded", true),
    (Col::Uploaded, "torrent.peers.col.uploaded", true),
    (Col::Country, "torrent.peers.col.country", false),
    (Col::Flags, "torrent.peers.col.flags", false),
];

/// Flag letters of `TorrentPeerVm::flags` with a legend entry, in legend order.
const FLAG_LETTERS: [char; 8] = ['D', 'U', 'I', 'P', 'E', 'X', 'H', 'S'];

/// Legend key of one flag letter (`torrent.peers.flag.<lowercase>`); `None` for letters we do not explain.
fn flag_key(c: char) -> Option<String> {
    let upper = c.to_ascii_uppercase();
    FLAG_LETTERS.contains(&upper).then(|| format!("torrent.peers.flag.{}", upper.to_ascii_lowercase()))
}

fn sort_peers(peers: &mut [TorrentPeerVm], sort: SortBy<Col>) {
    peers.sort_by(|a, b| {
        let ord = match sort.col {
            Col::Addr => a.addr.cmp(&b.addr),
            Col::Client => a.client.as_deref().unwrap_or("").to_lowercase().cmp(&b.client.as_deref().unwrap_or("").to_lowercase()),
            Col::Progress => cmp_f64(a.progress, b.progress),
            Col::Down => a.down_bps.cmp(&b.down_bps),
            Col::Up => a.up_bps.cmp(&b.up_bps),
            Col::Downloaded => a.downloaded_bytes.cmp(&b.downloaded_bytes),
            Col::Uploaded => a.uploaded_bytes.cmp(&b.uploaded_bytes),
            Col::Country => a.country.cmp(&b.country),
            Col::Flags => a.flags.cmp(&b.flags),
        };
        sort.apply(ord).then_with(|| a.addr.cmp(&b.addr))
    });
}

#[component]
pub fn TorrentPeersPage() -> Element {
    let ui = use_context::<Ui>();
    let peers = use_torrent_poll(POLL_FAST, |b, id| async move { b.torrent_peers(&id).await });
    let mut sort = use_signal(|| SortBy::new(Col::Down, true));

    let selected = ui.torrent.read().clone();
    let Some(id) = selected else {
        return rsx! {
            TorrentPicker {}
        };
    };

    let cur = sort();
    let content = match peers.for_id(&id) {
        None => rsx! {
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![85, 80, 90, 70] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState {
                error: e.clone(),
                title: Some(t("torrent.peers.error")),
                retry: e.kind != ErrorKind::NotWired,
                on_retry: move |_| peers.refresh(),
            }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "user".to_string(), title: t("torrent.peers.empty_title"), body: t("torrent.peers.empty_body") }
        },
        Some(Ok(mut list)) => {
            let summary = tf(
                "torrent.peers.summary",
                &[
                    ("n", &list.len().to_string()),
                    ("down", &format_speed(list.iter().map(|p| p.down_bps).sum())),
                    ("up", &format_speed(list.iter().map(|p| p.up_bps).sum())),
                ],
            );
            sort_peers(&mut list, cur);
            let [l_addr, l_client, l_progress, l_down, l_up, l_downloaded, l_uploaded, l_country, l_flags] =
                COLUMNS.map(|(_, k, _)| t(k));
            let unknown = t("torrent.peers.unknown");
            rsx! {
                p { class: "tk-ttable__summary tk-num", "{summary}" }
                div { class: "tk-ttable-wrap",
                    table { class: "tk-ttable tk-ttable--peers",
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
                            for p in list {
                                {
                                    let client = p.client.clone().unwrap_or_else(|| unknown.clone());
                                    let country = p.country.clone().unwrap_or_default();
                                    let flags: Vec<(usize, char, String)> = p
                                        .flags
                                        .chars()
                                        .filter(|c| !c.is_whitespace())
                                        .enumerate()
                                        .map(|(i, c)| (i, c, flag_key(c).map(|k| t(&k)).unwrap_or_default()))
                                        .collect();
                                    rsx! {
                                        tr { key: "{p.addr}",
                                            td { class: "tk-ttable__main tk-mono tk-selectable", "data-label": "{l_addr}", "{p.addr}" }
                                            td { "data-label": "{l_client}", title: "{client}", class: "tk-tpeers__client", "{client}" }
                                            td { "data-label": "{l_progress}",
                                                CellProgress { fraction: p.progress, label: p.addr.clone() }
                                            }
                                            td { "data-numeric": "true", "data-label": "{l_down}", class: "tk-num", {format_speed(p.down_bps)} }
                                            td { "data-numeric": "true", "data-label": "{l_up}", class: "tk-num", {format_speed(p.up_bps)} }
                                            td { "data-numeric": "true", "data-label": "{l_downloaded}", class: "tk-num", {format_bytes(p.downloaded_bytes)} }
                                            td { "data-numeric": "true", "data-label": "{l_uploaded}", class: "tk-num", {format_bytes(p.uploaded_bytes)} }
                                            td { "data-label": "{l_country}", class: "tk-mono",
                                                if country.is_empty() {
                                                    span { class: "tk-tpeers__none", {t("torrent.fmt.none")} }
                                                } else {
                                                    abbr { class: "tk-tpeers__country", title: "{country}", "{country}" }
                                                }
                                            }
                                            td { "data-label": "{l_flags}",
                                                span { class: "tk-tpeers__flags tk-mono",
                                                    for (i , c , legend) in flags {
                                                        abbr { key: "{i}", title: "{legend}", "{c}" }
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
        section { class: "tk-tpage tk-tpeers",
            {content}
            details { class: "tk-tpeers__legend",
                summary { {t("torrent.peers.legend")} }
                dl {
                    for c in FLAG_LETTERS {
                        div { key: "{c}", class: "tk-tpeers__legend-row",
                            dt { class: "tk-mono", "{c}" }
                            dd { {flag_key(c).map(|k| t(&k)).unwrap_or_default()} }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Lang, lookup};

    fn peer(addr: &str, down: u64, client: Option<&str>) -> TorrentPeerVm {
        TorrentPeerVm {
            addr: addr.to_owned(),
            client: client.map(str::to_owned),
            progress: 0.5,
            down_bps: down,
            up_bps: 0,
            downloaded_bytes: 0,
            uploaded_bytes: 0,
            country: None,
            flags: "DE".to_owned(),
        }
    }

    #[test]
    fn fastest_first_then_address() {
        let mut list = vec![peer("10.0.0.2:6881", 10, None), peer("10.0.0.1:6881", 10, Some("qB")), peer("10.0.0.3:6881", 99, None)];
        sort_peers(&mut list, SortBy::new(Col::Down, true));
        assert_eq!(list.iter().map(|p| p.addr.as_str()).collect::<Vec<_>>(), ["10.0.0.3:6881", "10.0.0.1:6881", "10.0.0.2:6881"]);
        sort_peers(&mut list, SortBy::new(Col::Client, false));
        assert_eq!(list[2].client.as_deref(), Some("qB"));
    }

    #[test]
    fn every_flag_letter_has_a_legend() {
        assert_eq!(flag_key('d').as_deref(), Some("torrent.peers.flag.d"));
        assert_eq!(flag_key('?'), None);
        for lang in [Lang::En, Lang::Es] {
            for c in FLAG_LETTERS {
                let k = flag_key(c).unwrap();
                assert_ne!(lookup(lang, &k), k, "{k}");
            }
        }
    }
}
