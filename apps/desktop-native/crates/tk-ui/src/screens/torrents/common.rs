//! Private helpers of the torrent client pages (owner: wave 3 C, docs/redesign/10 section 5.4). Declared from
//! `layout.rs` (`#[path]`), so `screens/torrents/mod.rs` stays as the contract left it.
//!
//! - Formatters: bytes / speeds in SI units (`1.38 GB`, `4.2 MB/s`), durations, percentages, all localized
//!   (`torrent.fmt.*`, decimal separator included).
//! - [`use_torrent_poll`]: polls one `Backend::torrent_*` call for the selected torrent (`Ui::torrent`) while the
//!   page is mounted; wakes early when the selection changes or the page asks for a refresh.
//! - Sortable table headers ([`SortTh`], [`SortBy`]) and the "choose a torrent" picker ([`TorrentPicker`]) the
//!   per-torrent pages show when nothing is selected.

use std::cmp::Ordering;
use std::future::Future;
use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, TorrentLibraryItemVm, TorrentStateVm, TrackerStatusVm, UiResult};
use crate::components::{EmptyState, ErrorState, Pill, SkeletonLines, Tone};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::ErrorKind;

/// Poll period of the live pages (Overview, Files, Peers).
pub(crate) const POLL_FAST: Duration = Duration::from_secs(1);
/// Poll period of slower data (Trackers, the shell's torrent list).
pub(crate) const POLL_SLOW: Duration = Duration::from_secs(3);
/// The poller sleeps in slices this long so a new selection or a refresh is picked up quickly.
const SLICE: Duration = Duration::from_millis(200);
/// A backend without torrents (`ErrorKind::NotWired`) is asked this many times less often.
const NOT_WIRED_BACKOFF: u32 = 5;

// ---------------------------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------------------------

const UNIT_KEYS: [&str; 5] = ["torrent.fmt.b", "torrent.fmt.kb", "torrent.fmt.mb", "torrent.fmt.gb", "torrent.fmt.tb"];

/// `bytes` scaled to SI units (1 KB = 1000 B): value and index into `UNIT_KEYS`.
fn scale(bytes: u64) -> (f64, usize) {
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNIT_KEYS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    (v, i)
}

/// Decimals shown for a scaled value: none for bytes, then 2 / 1 / 0 by magnitude (speeds at most 1).
fn decimals(v: f64, unit: usize, speed: bool) -> usize {
    if unit == 0 {
        0
    } else if v < 10.0 && !speed {
        2
    } else if v < 100.0 {
        1
    } else {
        0
    }
}

/// `v` with `d` decimals and the language's decimal separator.
fn number(v: f64, d: usize) -> String {
    let s = format!("{v:.d$}");
    let sep = t("torrent.fmt.decimal");
    if sep == "." { s } else { s.replace('.', &sep) }
}

fn sized(bytes: u64, speed: bool) -> String {
    let (v, i) = scale(bytes);
    tf("torrent.fmt.size", &[("n", &number(v, decimals(v, i, speed))), ("unit", &t(UNIT_KEYS[i]))])
}

/// `1.38 GB`, `512 B`.
pub(crate) fn format_bytes(bytes: u64) -> String {
    sized(bytes, false)
}

/// `4.2 MB/s`.
pub(crate) fn format_speed(bps: u64) -> String {
    tf("torrent.fmt.speed", &[("size", &sized(bps, true))])
}

/// `42 s`, `3 min 05 s`, `1 h 02 min`, `2 d 4 h`.
pub(crate) fn format_duration(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60, secs % 60);
    match secs {
        0..60 => tf("torrent.fmt.dur_s", &[("s", &s.to_string())]),
        60..3_600 => tf("torrent.fmt.dur_ms", &[("m", &m.to_string()), ("s", &format!("{s:02}"))]),
        3_600..86_400 => tf("torrent.fmt.dur_hm", &[("h", &(secs / 3_600).to_string()), ("m", &format!("{m:02}"))]),
        _ => tf("torrent.fmt.dur_dh", &[("d", &d.to_string()), ("h", &h.to_string())]),
    }
}

/// `fraction` 0..=1 as `42.5%` (one decimal under 100, never rounds an incomplete file up to 100).
pub(crate) fn format_percent(fraction: f64) -> String {
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let text = if f >= 1.0 {
        "100".to_owned()
    } else {
        // Cap at 99.9 so 99.97 % does not read 100.0 % while pieces are still missing.
        number(((f * 1000.0).round() / 10.0).min(99.9), 1)
    };
    tf("torrent.fmt.percent", &[("n", &text)])
}

/// An optional count (`None` = the tracker did not say).
pub(crate) fn format_opt(n: Option<u32>) -> String {
    n.map(|n| n.to_string()).unwrap_or_else(|| t("torrent.fmt.none"))
}

/// Upload / download ratio, two decimals; `None` before anything was downloaded.
pub(crate) fn ratio(uploaded: u64, downloaded: u64) -> Option<String> {
    (downloaded > 0).then(|| number(uploaded as f64 / downloaded as f64, 2))
}

// ---------------------------------------------------------------------------------------------
// Tones
// ---------------------------------------------------------------------------------------------

pub(crate) fn state_tone(state: TorrentStateVm) -> Tone {
    match state {
        TorrentStateVm::Initializing => Tone::Warn,
        TorrentStateVm::Downloading => Tone::Primary,
        TorrentStateVm::Seeding => Tone::Ok,
        TorrentStateVm::Paused => Tone::Neutral,
        TorrentStateVm::Error => Tone::Err,
    }
}

pub(crate) fn tracker_tone(status: TrackerStatusVm) -> Tone {
    match status {
        TrackerStatusVm::Working => Tone::Ok,
        TrackerStatusVm::Updating => Tone::Warn,
        TrackerStatusVm::Failed => Tone::Err,
        TrackerStatusVm::Unknown => Tone::Neutral,
    }
}

/// Localized state pill of a torrent (`torrent.state.<id>`).
#[component]
pub(crate) fn StatePill(state: TorrentStateVm) -> Element {
    rsx! {
        span { class: "tk-tstate", "data-state": "{state.id()}",
            Pill { text: t(&format!("torrent.state.{}", state.id())), tone: state_tone(state) }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Search and sorting
// ---------------------------------------------------------------------------------------------

/// Case-insensitive match of every whitespace-separated word of `query` against any of `fields`.
pub(crate) fn matches_query(query: &str, fields: &[&str]) -> bool {
    let hay: Vec<String> = fields.iter().map(|f| f.to_lowercase()).collect();
    query.split_whitespace().map(str::to_lowercase).all(|w| hay.iter().any(|h| h.contains(&w)))
}

/// Sort state of a table: column and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortBy<C: Copy + PartialEq> {
    pub col: C,
    pub desc: bool,
}

impl<C: Copy + PartialEq> SortBy<C> {
    pub(crate) fn new(col: C, desc: bool) -> Self {
        Self { col, desc }
    }

    /// Header click: the same column flips the direction; another starts at `desc_first` (numbers start high).
    pub(crate) fn click(self, col: C, desc_first: bool) -> Self {
        if self.col == col { Self { col, desc: !self.desc } } else { Self { col, desc: desc_first } }
    }

    pub(crate) fn apply(self, ord: Ordering) -> Ordering {
        if self.desc { ord.reverse() } else { ord }
    }

    pub(crate) fn aria(self, col: C) -> &'static str {
        match (self.col == col, self.desc) {
            (false, _) => "none",
            (true, false) => "ascending",
            (true, true) => "descending",
        }
    }
}

/// `f64` ordering for progress columns (NaN sorts first).
pub(crate) fn cmp_f64(a: f64, b: f64) -> Ordering {
    a.partial_cmp(&b).unwrap_or_else(|| a.is_nan().cmp(&b.is_nan()).reverse())
}

/// Sortable column header. `numeric` right-aligns it (and its cells, via the same `data-numeric`).
#[component]
pub(crate) fn SortTh(
    label: String,
    /// `aria-sort` value from [`SortBy::aria`].
    sort: &'static str,
    #[props(default)] numeric: bool,
    #[props(default)] on_sort: EventHandler<()>,
) -> Element {
    let icon = match sort {
        "ascending" => "chevron-up",
        "descending" => "chevron-down",
        _ => "",
    };
    rsx! {
        th { class: "tk-ttable__th", "scope": "col", "aria-sort": "{sort}", "data-numeric": "{numeric}",
            button { class: "tk-ttable__sort", r#type: "button", onclick: move |_| on_sort.call(()),
                span { "{label}" }
                if !icon.is_empty() {
                    Icon { name: icon.to_string(), size: 16 }
                }
            }
        }
    }
}

/// Thin determinate bar for table cells (progress of a file / peer) with the percentage next to it.
#[component]
pub(crate) fn CellProgress(fraction: f64, #[props(default)] label: String) -> Element {
    let f = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let width = format!("{:.2}%", f * 100.0);
    let text = format_percent(f);
    let done = f >= 1.0;
    let now = (f * 100.0).round();
    rsx! {
        span { class: "tk-tbar", "data-done": "{done}",
            span {
                class: "tk-tbar__track",
                role: "progressbar",
                "aria-valuenow": "{now}",
                "aria-valuemin": "0",
                "aria-valuemax": "100",
                "aria-label": "{label}",
                span { class: "tk-tbar__fill", style: "width:{width}" }
            }
            span { class: "tk-tbar__text tk-num", "{text}" }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Polling
// ---------------------------------------------------------------------------------------------

/// One answer of a poll, tagged with the torrent it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Polled<T> {
    pub key: String,
    pub value: UiResult<T>,
}

/// Handle returned by [`use_torrent_poll`].
pub(crate) struct Poll<T: 'static> {
    data: Signal<Option<Polled<T>>>,
    nudge: Signal<u64>,
}

impl<T: 'static> Clone for Poll<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: 'static> Copy for Poll<T> {}

impl<T: Clone + 'static> Poll<T> {
    /// The latest answer for torrent `id`; `None` while the first answer for it is pending (loading state).
    pub(crate) fn for_id(&self, id: &str) -> Option<UiResult<T>> {
        self.data.read().as_ref().filter(|p| p.key == id).map(|p| p.value.clone())
    }

    /// Ask again now (after an action, or Retry).
    pub(crate) fn refresh(mut self) {
        let n = *self.nudge.peek();
        self.nudge.set(n.wrapping_add(1));
    }
}

/// Poll `fetch(backend, selected_id)` every `every` while the calling page is mounted (`use_future` is dropped
/// with the component). Nothing is fetched while no torrent is selected. An answer that arrives after the
/// selection moved on is dropped; identical answers do not re-render.
pub(crate) fn use_torrent_poll<T, F, Fut>(every: Duration, fetch: F) -> Poll<T>
where
    T: PartialEq + 'static,
    F: Fn(BackendRef, String) -> Fut + Clone + 'static,
    Fut: Future<Output = UiResult<T>> + 'static,
{
    let backend = use_context::<Services>().backend;
    let ui = use_context::<Ui>();
    let data = use_signal(|| None::<Polled<T>>);
    let nudge = use_signal(|| 0u64);
    use_future(move || {
        let backend = backend.clone();
        let fetch = fetch.clone();
        let mut data = data;
        async move {
            loop {
                let id = ui.torrent.peek().clone();
                let seen = *nudge.peek();
                let mut slices = (every.as_millis() / SLICE.as_millis()).max(1) as u32;
                if let Some(id) = id.clone() {
                    let value = fetch(backend.clone(), id.clone()).await;
                    if matches!(&value, Err(e) if e.kind == ErrorKind::NotWired) {
                        slices *= NOT_WIRED_BACKOFF;
                    }
                    if ui.torrent.peek().as_deref() == Some(id.as_str()) {
                        let next = Polled { key: id, value };
                        if data.peek().as_ref() != Some(&next) {
                            data.set(Some(next));
                        }
                    }
                }
                for _ in 0..slices {
                    tokio::time::sleep(SLICE).await;
                    if *ui.torrent.peek() != id || *nudge.peek() != seen {
                        break;
                    }
                }
            }
        }
    });
    Poll { data, nudge }
}

/// The torrent list (`torrent_library`), polled every [`POLL_SLOW`] while mounted (shell header + picker).
pub(crate) fn use_torrent_list() -> Signal<Option<UiResult<Vec<TorrentLibraryItemVm>>>> {
    let backend = use_context::<Services>().backend;
    let list = use_signal(|| None::<UiResult<Vec<TorrentLibraryItemVm>>>);
    use_future(move || {
        let backend = backend.clone();
        let mut list = list;
        async move {
            loop {
                let next = backend.torrent_library().await;
                let not_wired = matches!(&next, Err(e) if e.kind == ErrorKind::NotWired);
                if list.peek().as_ref() != Some(&next) {
                    list.set(Some(next));
                }
                let wait = if not_wired { POLL_SLOW * NOT_WIRED_BACKOFF } else { POLL_SLOW };
                tokio::time::sleep(wait).await;
            }
        }
    });
    list
}

// ---------------------------------------------------------------------------------------------
// No torrent selected
// ---------------------------------------------------------------------------------------------

/// Shown by the per-torrent pages while `Ui::torrent` is `None`: the session's torrents to pick from, or an
/// empty state that leads to the Library.
#[component]
pub(crate) fn TorrentPicker() -> Element {
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let list = use_torrent_list();
    let mut selected = ui.torrent;

    match &*list.read() {
        None => rsx! {
            section { class: "tk-panel", "aria-busy": "true",
                SkeletonLines { widths: vec![70, 55, 62] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: t("torrent.shell.list_error"), retry: false }
        },
        Some(Ok(items)) if items.is_empty() => rsx! {
            EmptyState {
                icon: "magnet".to_string(),
                title: t("torrent.shell.empty_title"),
                body: t("torrent.shell.empty_body"),
                action_label: t("torrent.shell.open_library"),
                on_action: move |_| {
                    navigator.push(Route::TorrentLibraryPage {});
                },
            }
        },
        Some(Ok(items)) => {
            let items = items.clone();
            rsx! {
                section { class: "tk-panel tk-tpick",
                    h2 { class: "tk-panel__title",
                        Icon { name: "magnet".to_string(), size: 24 }
                        {t("torrent.shell.pick_title")}
                    }
                    p { class: "tk-panel__hint", {t("torrent.shell.pick_body")} }
                    ul { class: "tk-tpick__list",
                        for item in items {
                            {
                                let id = item.id.clone();
                                let size = format_bytes(item.size_bytes);
                                let pct = format_percent(item.progress);
                                let series = item.series.clone().unwrap_or_default();
                                rsx! {
                                    li { key: "{item.id}",
                                        button {
                                            class: "tk-tpick__item",
                                            r#type: "button",
                                            onclick: move |_| selected.set(Some(id.clone())),
                                            span { class: "tk-tpick__main",
                                                span { class: "tk-tpick__name", "{item.name}" }
                                                if !series.is_empty() {
                                                    span { class: "tk-tpick__sub", "{series}" }
                                                }
                                            }
                                            span { class: "tk-tpick__meta tk-num", "{pct} · {size}" }
                                            StatePill { state: item.state }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_and_speeds_use_si_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_380_000_000), "1.38 GB");
        assert_eq!(format_bytes(45_600_000), "45.6 MB");
        assert_eq!(format_bytes(712_000_000), "712 MB");
        assert_eq!(format_speed(4_200_000), "4.2 MB/s");
        assert_eq!(format_speed(0), "0 B/s");
        assert_eq!(format_bytes(u64::MAX).split(' ').nth(1), Some("TB"));
    }

    #[test]
    fn durations_and_percentages() {
        assert_eq!(format_duration(42), "42 s");
        assert_eq!(format_duration(185), "3 min 05 s");
        assert_eq!(format_duration(3_720), "1 h 02 min");
        assert_eq!(format_duration(2 * 86_400 + 4 * 3_600), "2 d 4 h");
        assert_eq!(format_percent(0.425), "42.5%");
        assert_eq!(format_percent(0.99999), "99.9%");
        assert_eq!(format_percent(1.0), "100%");
        assert_eq!(format_percent(f64::NAN), "0.0%");
        assert_eq!(ratio(10, 0), None);
        assert_eq!(ratio(150, 100).as_deref(), Some("1.50"));
    }

    #[test]
    fn query_matches_every_word() {
        assert!(matches_query("", &["anything"]));
        assert!(matches_query("frieren 05", &["[SubsPlease] Sousou no Frieren - 05 (1080p).mkv"]));
        assert!(!matches_query("frieren 06", &["[SubsPlease] Sousou no Frieren - 05 (1080p).mkv"]));
        assert!(matches_query("extras", &["NCOP.mkv", "Season 1/Extras/NCOP.mkv"]));
    }

    #[test]
    fn sort_click_flips_or_starts_fresh() {
        let s = SortBy::new(1u8, false);
        assert_eq!(s.click(1, true), SortBy::new(1, true));
        assert_eq!(s.click(2, true), SortBy::new(2, true));
        assert_eq!(s.aria(1), "ascending");
        assert_eq!(s.aria(2), "none");
        assert_eq!(SortBy::new(1u8, true).apply(Ordering::Less), Ordering::Greater);
        assert_eq!(cmp_f64(0.2, 0.5), Ordering::Less);
    }
}
