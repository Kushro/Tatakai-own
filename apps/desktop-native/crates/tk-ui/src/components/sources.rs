//! `sources.rs`: SourceRow, SourcePicker (sheet) and SourceSummary (Details side panel), docs/redesign/07
//! section 3.4 and the prototype `sources.js`. Props are the wave 1 contract (docs/redesign/08 section 6).
//!
//! Unlike the other components, `SourceSummary` loads its own data: it reads the backend from the
//! `Services` context (`use_context::<Services>().backend`, a `BackendRef`) and the prefs from the `Ui`
//! context (`ui.settings.read().sources`). Its resource does not track prop changes, so callers key it
//! per episode: `SourceSummary { key: "{id}-{ep}", .. }`.
//!
//! Wave 3 (docs/redesign/10 section 5.6, owner E): each row shows its release tags as chips (SeaDex best, group,
//! BD / WEB / REMUX, resolution, codec, bit depth, dual audio, multi-sub, batch) and the picker has a filter bar
//! (kind segment + toggle chips for release source, resolution, codec, 10-bit, dual audio, multi-sub, batch, SeaDex
//! best and a reliability floor). Filter state is local to the picker ([`SourceFilters`], pure, tested below) and
//! filtering never reorders: the list stays in ranking order.

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{ReleaseSourceVm, SourceKindVm, SourceVm, VideoCodecVm};
use crate::components::button::{Button, ButtonSize, ButtonVariant, IconButton, IconButtonVariant};
use crate::components::chips::{Banner, Chip, ChipKind, InfoChip, Pill, Tone};
use crate::components::segmented::{SegOption, Segmented};
use crate::components::sheet::Sheet;
use crate::components::state::{EmptyState, ErrorState};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{SourceModeVm, SourcePrefsVm};

/// Reliability bar tone (`data-tone`): >= 0.9 ok, >= 0.7 warn, else err.
pub fn reliability_tone(r: f32) -> &'static str {
    if r >= 0.9 {
        "ok"
    } else if r >= 0.7 {
        "warn"
    } else {
        "err"
    }
}

fn reliability_key(r: f32) -> &'static str {
    match reliability_tone(r) {
        "ok" => "src.rel.high",
        "warn" => "src.rel.mid",
        _ => "src.rel.low",
    }
}

/// `1_450_000_000` -> `1.4 GB`, `350_000_000` -> `350 MB` (decimal units, like torrent indexes).
pub fn format_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1.0 { format!("{gb:.1} GB") } else { format!("{:.0} MB", bytes as f64 / 1e6) }
}

/// `es, en` -> `ES · EN`; empty -> "none".
fn langs(list: &[String]) -> String {
    if list.is_empty() {
        t("src.none")
    } else {
        list.iter().map(|l| l.to_uppercase()).collect::<Vec<_>>().join(" · ")
    }
}

/// Key/value metadata of a row (web: server, quality, latency, subs, dub; torrent: group, quality, size,
/// seeders, leechers, subs, dub).
fn row_meta(s: &SourceVm) -> Vec<(String, String)> {
    let quality = [s.quality.as_str(), s.container.as_deref().unwrap_or("")].join(" ").trim().to_owned();
    let mut out = Vec::new();
    match s.kind {
        SourceKindVm::Web => {
            out.push((t("src.server"), s.label.clone()));
            if !quality.is_empty() {
                out.push((t("player.quality"), quality));
            }
            if let Some(ms) = s.latency_ms {
                out.push((t("src.latency"), format!("{ms} ms")));
            }
        }
        SourceKindVm::Torrent => {
            // quality / container / codec are tag chips on torrent rows (`tag_chips`)
            out.push((t("src.group"), s.label.clone()));
            if let Some(b) = s.size_bytes {
                out.push((t("src.size"), format_size(b)));
            }
            if let Some(n) = s.seeders {
                out.push((t("src.seeders"), n.to_string()));
            }
            if let Some(n) = s.leechers {
                out.push((t("src.leechers"), n.to_string()));
            }
        }
    }
    out.push((t("src.subs"), langs(&s.subs)));
    out.push((t("src.dubs"), langs(&s.dubs)));
    out
}

/// Release-tag chips of a row, in display order, as `(text, tone)`; the tone is the `data-tone` of `.tk-stag`
/// (`best` = SeaDex best, `batch`, `group`, empty = plain). Technical codes (`BD`, `HEVC`, `1080p`) are not
/// translated. Web rows only get chips when their tags say something (they are empty today).
fn tag_chips(s: &SourceVm) -> Vec<(String, &'static str)> {
    let tags = &s.tags;
    let torrent = s.kind == SourceKindVm::Torrent;
    let mut out: Vec<(String, &'static str)> = Vec::new();
    if tags.seadex_best {
        out.push((t("src.tag.seadex_best"), "best"));
    }
    if let Some(g) = tags.group.as_ref().filter(|g| !g.is_empty() && **g != s.label) {
        out.push((g.clone(), "group"));
    }
    if let Some(src) = tags.source {
        out.push((src.label().to_owned(), ""));
    }
    if torrent && !s.quality.is_empty() {
        out.push((s.quality.clone(), ""));
    }
    if let Some(c) = tags.codec {
        out.push((c.label().to_owned(), ""));
    } else if torrent && let Some(c) = s.container.as_ref().filter(|c| !c.is_empty()) {
        out.push((c.clone(), ""));
    }
    if let Some(d) = tags.bit_depth {
        out.push((tf("src.tag.bit_depth", &[("n", &d.to_string())]), ""));
    }
    if tags.dual_audio {
        out.push((t("src.tag.dual_audio"), ""));
    }
    if tags.multi_sub {
        out.push((t("src.tag.multi_sub"), ""));
    }
    if tags.batch {
        let text = match tags.episodes {
            Some((a, b)) if b > a => tf("src.tag.batch_range", &[("from", &a.to_string()), ("to", &b.to_string())]),
            _ => t("src.tag.batch"),
        };
        out.push((text, "batch"));
    }
    out
}

/// Resolutions the picker can filter by, best first (`SourceVm::quality` values).
const FILTER_RESOLUTIONS: [&str; 5] = ["2160p", "1440p", "1080p", "720p", "480p"];

/// Reliability floor of the picker filter, on the `reliability_tone` thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelFloor {
    /// `>= 0.7` (medium or high).
    Mid,
    /// `>= 0.9`.
    High,
}

impl RelFloor {
    fn min(self) -> f32 {
        match self {
            Self::Mid => 0.7,
            Self::High => 0.9,
        }
    }
}

/// Filter state of the source picker. Empty lists = no constraint (a list is "any of"); a `true` flag = "must have".
/// Filtering keeps the ranked order.
#[derive(Debug, Clone, PartialEq, Default)]
struct SourceFilters {
    kind: Option<SourceKindVm>,
    sources: Vec<ReleaseSourceVm>,
    resolutions: Vec<&'static str>,
    codecs: Vec<VideoCodecVm>,
    ten_bit: bool,
    dual_audio: bool,
    multi_sub: bool,
    batch: bool,
    seadex_best: bool,
    min_reliability: Option<RelFloor>,
}

impl SourceFilters {
    fn matches(&self, s: &SourceVm) -> bool {
        let tags = &s.tags;
        self.kind.is_none_or(|k| s.kind == k)
            && (self.sources.is_empty() || tags.source.is_some_and(|x| self.sources.contains(&x)))
            && (self.resolutions.is_empty() || self.resolutions.iter().any(|q| *q == s.quality))
            && (self.codecs.is_empty() || tags.codec.is_some_and(|x| self.codecs.contains(&x)))
            && (!self.ten_bit || tags.bit_depth == Some(10))
            && (!self.dual_audio || tags.dual_audio)
            && (!self.multi_sub || tags.multi_sub)
            && (!self.batch || tags.batch)
            && (!self.seadex_best || tags.seadex_best)
            && self.min_reliability.is_none_or(|f| s.reliability >= f.min())
    }

    /// The sources that pass, in the given (ranked) order.
    fn apply(&self, list: &[SourceVm]) -> Vec<SourceVm> {
        list.iter().filter(|s| self.matches(s)).cloned().collect()
    }

    /// Active chip filters (the kind segment is not counted: it is always visible).
    fn active(&self) -> usize {
        let flags = [self.ten_bit, self.dual_audio, self.multi_sub, self.batch, self.seadex_best];
        self.sources.len()
            + self.resolutions.len()
            + self.codecs.len()
            + flags.iter().filter(|f| **f).count()
            + usize::from(self.min_reliability.is_some())
    }

    fn toggle_floor(&mut self, floor: RelFloor) {
        self.min_reliability = if self.min_reliability == Some(floor) { None } else { Some(floor) };
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, v: T) {
    match list.iter().position(|x| *x == v) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(v),
    }
}

/// What the current list offers, so the filter bar only draws chips that can match something.
#[derive(Debug, Clone, PartialEq, Default)]
struct FilterOptions {
    sources: Vec<ReleaseSourceVm>,
    resolutions: Vec<&'static str>,
    codecs: Vec<VideoCodecVm>,
    ten_bit: bool,
    dual_audio: bool,
    multi_sub: bool,
    batch: bool,
    seadex_best: bool,
}

impl FilterOptions {
    fn of(list: &[SourceVm]) -> Self {
        Self {
            sources: ReleaseSourceVm::ALL.into_iter().filter(|x| list.iter().any(|s| s.tags.source == Some(*x))).collect(),
            resolutions: FILTER_RESOLUTIONS.into_iter().filter(|q| list.iter().any(|s| s.quality == *q)).collect(),
            codecs: VideoCodecVm::ALL.into_iter().filter(|x| list.iter().any(|s| s.tags.codec == Some(*x))).collect(),
            ten_bit: list.iter().any(|s| s.tags.bit_depth == Some(10)),
            dual_audio: list.iter().any(|s| s.tags.dual_audio),
            multi_sub: list.iter().any(|s| s.tags.multi_sub),
            batch: list.iter().any(|s| s.tags.batch),
            seadex_best: list.iter().any(|s| s.tags.seadex_best),
        }
    }

    fn any_feature(&self) -> bool {
        self.ten_bit || self.dual_audio || self.multi_sub || self.batch || self.seadex_best
    }
}

/// One source (radio row). `on_pick` receives `source.id`. Disabled when the extension is off.
#[component]
pub fn SourceRow(source: SourceVm, selected: bool, on_pick: EventHandler<String>) -> Element {
    let id = source.id.clone();
    let kind = source.kind.id();
    let disabled = !source.ext_enabled;
    let icon = match source.kind {
        SourceKindVm::Web => "language",
        SourceKindVm::Torrent => "download",
    };
    let name = format!("{} · {}", source.ext_name, source.label);
    let kind_text = t(&format!("src.{kind}"));
    let off_text = tf("src.ext_off", &[("name", &source.ext_name)]);
    let meta = row_meta(&source);
    let chips = tag_chips(&source);
    // Wave B: where the row comes from (j: `server:<id>` / `stremio:<id>` / extension id) and whether it is an embed
    // page the sniffer resolves (k, `sources.embed-sniffer.ranking`).
    let origin = source.ext_id.clone();
    let embed = source.embed;
    let pct = (source.reliability.clamp(0.0, 1.0) * 100.0).round() as u32;
    let tone = reliability_tone(source.reliability);
    let rel_text = format!("{}: {} · {pct} %", t("src.reliability"), t(reliability_key(source.reliability)));
    // Wave A, worker i: debrid cache status of a torrent row (`SourceVm::debrid`).
    let debrid_attr = match source.debrid {
        Some(d) if d.cached => "cached",
        Some(_) => "not_cached",
        None => "none",
    };
    let debrid_pill = source.debrid.map(|d| {
        let key = if d.cached { "debrid.cached" } else { "debrid.not_cached" };
        (tf(key, &[("service", d.service.label())]), d.cached)
    });
    rsx! {
        button {
            class: "tk-srow",
            r#type: "button",
            role: "radio",
            "aria-checked": "{selected}",
            "data-kind": "{kind}",
            "data-debrid": "{debrid_attr}",
            "data-origin": "{origin}",
            "data-embed": "{embed}",
            disabled,
            onclick: move |_| on_pick.call(id.clone()),
            span { class: "tk-srow__ico", Icon { name: icon.to_string(), size: 20 } }
            span { class: "tk-srow__main",
                span { class: "tk-srow__title",
                    b { "{name}" }
                    Pill { text: kind_text, tone: Tone::Neutral, plain: true }
                    if embed {
                        Pill { text: t("sniff.chip"), tone: Tone::Warn, plain: true }
                    }
                    if source.recommended {
                        Pill { text: t("src.recommended"), tone: Tone::Primary }
                    }
                    if let Some((text, cached)) = debrid_pill {
                        Pill { text, tone: if cached { Tone::Ok } else { Tone::Neutral }, plain: !cached }
                    }
                    if disabled {
                        Pill { text: off_text, tone: Tone::Err, plain: true }
                    }
                    if let Some(note) = source.note.clone() {
                        span { class: "tk-srow__note", "{note}" }
                    }
                }
                if !chips.is_empty() {
                    span { class: "tk-srow__tags",
                        for (i , (text , chip_tone)) in chips.into_iter().enumerate() {
                            span { key: "{i}", class: "tk-stag", "data-tone": "{chip_tone}",
                                if chip_tone == "best" {
                                    Icon { name: "star".to_string(), size: 14 }
                                }
                                "{text}"
                            }
                        }
                    }
                }
                span { class: "tk-srow__meta",
                    for (k , v) in meta {
                        span { key: "{k}", class: "tk-srow__kv",
                            span { class: "tk-srow__k", "{k}" }
                            span { class: "tk-srow__v", "{v}" }
                        }
                    }
                }
                if let Some(hash) = source.info_hash.clone() {
                    span { class: "tk-srow__hash", "{hash}" }
                }
                span { class: "tk-srow__rel",
                    span { class: "tk-srow__relbar", "aria-hidden": "true",
                        i { style: "width:{pct}%", "data-tone": "{tone}" }
                    }
                    span { class: "tk-srow__rel-label", "{rel_text}" }
                }
            }
            if selected {
                span { class: "tk-srow__check", Icon { name: "check".to_string(), size: 20 } }
            }
        }
    }
}

/// All sources of an episode in a side sheet, already ranked (`sources[0]` is the recommended one).
/// Render it always; it shows nothing while `open` is false. Picking calls `on_pick(id)`; the caller
/// closes it. The checked row is the `picked` one (or the recommended one when nothing is picked).
/// With `on_download`, torrent rows of enabled extensions also get a "Download for later" button that
/// calls `on_download(id)` (background download, `Backend::torrent_add_download`).
#[component]
pub fn SourcePicker(
    open: bool,
    sources: Vec<SourceVm>,
    prefs: SourcePrefsVm,
    on_pick: EventHandler<String>,
    on_close: EventHandler<()>,
    #[props(default)] on_download: Option<EventHandler<String>>,
) -> Element {
    let mut filters = use_signal(SourceFilters::default);
    let mut show_filters = use_signal(|| false);
    let navigator = use_navigator();
    if !open {
        return rsx! {};
    }
    let f = filters();
    let kind = f.kind.map_or("all", |k| k.id()).to_string();
    let any_picked = sources.iter().any(|s| s.picked);
    let mut ranked = sources.clone();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let opts = FilterOptions::of(&ranked);
    let shown: Vec<SourceVm> = f.apply(&ranked);
    let active = f.active();
    let filtered = f != SourceFilters::default();
    let filters_open = show_filters();
    let filters_label = if active == 0 { t("src.filter.title") } else { tf("src.filter.title_n", &[("n", &active.to_string())]) };
    let count_text = tf("src.filter.count", &[("shown", &shown.len().to_string()), ("total", &ranked.len().to_string())]);
    let manual = prefs.mode == SourceModeVm::Manual;
    let banner_text = if manual && !any_picked {
        t("src.choose_first")
    } else {
        format!("{} · {}", t(&format!("src.mode.{}", prefs.mode.id())), t("src.failover"))
    };
    let banner_icon = if manual { "tune" } else { "enhance" };
    let options: Vec<SegOption> = vec![
        ("all".to_string(), t("src.filter.all")),
        ("web".to_string(), t("src.web")),
        ("torrent".to_string(), t("src.torrent")),
    ];
    let sub_chip = format!("{}: {}", t("src.sub_lang"), prefs.sub_lang.to_uppercase());
    let dub_chip = format!("{}: {}", t("src.dub_lang"), prefs.dub_lang.to_uppercase());
    let quality_chip = format!("≥ {}", prefs.min_quality.id());
    let prefer_chip = prefs.prefer.map(|k| format!("{}: {}", t("src.prefer"), t(&format!("src.{}", k.id()))));
    let list_label = t("src.title");
    let download_label = t("src.download");
    rsx! {
        Sheet { title: t("src.title"), wide: true, on_close: move |_| on_close.call(()),
            div { class: "tk-spicker",
                Banner { text: banner_text, icon: banner_icon.to_string(), tone: Tone::Primary,
                    Button {
                        label: t("settings.title"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Xs,
                        onclick: move |_| {
                            on_close.call(());
                            navigator.push(Route::SettingsPage {});
                        },
                    }
                }
                Segmented {
                    options,
                    selected: kind.clone(),
                    label: t("src.title"),
                    small: true,
                    on_select: move |k: String| {
                        filters.write().kind = SourceKindVm::ALL.into_iter().find(|x| x.id() == k);
                    },
                }
                if !ranked.is_empty() {
                    div { class: "tk-sfilters",
                        div { class: "tk-sfilters__bar",
                            Chip {
                                label: filters_label,
                                icon: "filter".to_string(),
                                on: filters_open,
                                onclick: move |_| show_filters.set(!filters_open),
                            }
                            if filtered {
                                Chip {
                                    label: t("src.filter.clear"),
                                    kind: ChipKind::Assist,
                                    icon: "close".to_string(),
                                    onclick: move |_| filters.set(SourceFilters::default()),
                                }
                            }
                            span { class: "tk-sfilters__count", "aria-live": "polite", "{count_text}" }
                        }
                        if filters_open {
                            div { class: "tk-sfilters__groups",
                                if !opts.sources.is_empty() {
                                    div { class: "tk-sfilters__group",
                                        span { class: "tk-sfilters__label", {t("src.filter.group.source")} }
                                        div { class: "tk-chips",
                                            for src in opts.sources.iter().copied() {
                                                Chip {
                                                    key: "{src:?}",
                                                    label: src.label().to_string(),
                                                    on: f.sources.contains(&src),
                                                    onclick: move |_| toggle(&mut filters.write().sources, src),
                                                }
                                            }
                                        }
                                    }
                                }
                                if !opts.resolutions.is_empty() {
                                    div { class: "tk-sfilters__group",
                                        span { class: "tk-sfilters__label", {t("src.filter.group.resolution")} }
                                        div { class: "tk-chips",
                                            for q in opts.resolutions.iter().copied() {
                                                Chip {
                                                    key: "{q}",
                                                    label: q.to_string(),
                                                    on: f.resolutions.contains(&q),
                                                    onclick: move |_| toggle(&mut filters.write().resolutions, q),
                                                }
                                            }
                                        }
                                    }
                                }
                                if !opts.codecs.is_empty() {
                                    div { class: "tk-sfilters__group",
                                        span { class: "tk-sfilters__label", {t("src.filter.group.codec")} }
                                        div { class: "tk-chips",
                                            for c in opts.codecs.iter().copied() {
                                                Chip {
                                                    key: "{c:?}",
                                                    label: c.label().to_string(),
                                                    on: f.codecs.contains(&c),
                                                    onclick: move |_| toggle(&mut filters.write().codecs, c),
                                                }
                                            }
                                        }
                                    }
                                }
                                if opts.any_feature() {
                                    div { class: "tk-sfilters__group",
                                        span { class: "tk-sfilters__label", {t("src.filter.group.features")} }
                                        div { class: "tk-chips",
                                            if opts.seadex_best {
                                                Chip {
                                                    label: t("src.tag.seadex_best"),
                                                    on: f.seadex_best,
                                                    onclick: move |_| {
                                                        let mut w = filters.write();
                                                        w.seadex_best = !w.seadex_best;
                                                    },
                                                }
                                            }
                                            if opts.ten_bit {
                                                Chip {
                                                    label: tf("src.tag.bit_depth", &[("n", "10")]),
                                                    on: f.ten_bit,
                                                    onclick: move |_| {
                                                        let mut w = filters.write();
                                                        w.ten_bit = !w.ten_bit;
                                                    },
                                                }
                                            }
                                            if opts.dual_audio {
                                                Chip {
                                                    label: t("src.tag.dual_audio"),
                                                    on: f.dual_audio,
                                                    onclick: move |_| {
                                                        let mut w = filters.write();
                                                        w.dual_audio = !w.dual_audio;
                                                    },
                                                }
                                            }
                                            if opts.multi_sub {
                                                Chip {
                                                    label: t("src.tag.multi_sub"),
                                                    on: f.multi_sub,
                                                    onclick: move |_| {
                                                        let mut w = filters.write();
                                                        w.multi_sub = !w.multi_sub;
                                                    },
                                                }
                                            }
                                            if opts.batch {
                                                Chip {
                                                    label: t("src.tag.batch"),
                                                    on: f.batch,
                                                    onclick: move |_| {
                                                        let mut w = filters.write();
                                                        w.batch = !w.batch;
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                                div { class: "tk-sfilters__group",
                                    span { class: "tk-sfilters__label", {t("src.filter.group.reliability")} }
                                    div { class: "tk-chips",
                                        Chip {
                                            label: t("src.filter.rel_mid"),
                                            on: f.min_reliability == Some(RelFloor::Mid),
                                            onclick: move |_| filters.write().toggle_floor(RelFloor::Mid),
                                        }
                                        Chip {
                                            label: t("src.filter.rel_high"),
                                            on: f.min_reliability == Some(RelFloor::High),
                                            onclick: move |_| filters.write().toggle_floor(RelFloor::High),
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "tk-chips",
                    InfoChip { text: sub_chip, icon: "cc".to_string() }
                    InfoChip { text: dub_chip, icon: "audio".to_string() }
                    InfoChip { text: quality_chip, icon: "hd".to_string() }
                    if let Some(p) = prefer_chip {
                        InfoChip { text: p, icon: "filter".to_string() }
                    }
                }
                if ranked.is_empty() {
                    p { class: "tk-muted", {t("src.none")} }
                } else if shown.is_empty() {
                    EmptyState {
                        icon: "filter".to_string(),
                        title: t("src.filter.empty.title"),
                        body: t("src.filter.empty.body"),
                        action_label: t("src.filter.clear"),
                        on_action: move |_| filters.set(SourceFilters::default()),
                    }
                } else {
                    div { class: "tk-slist", role: "radiogroup", "aria-label": "{list_label}",
                        for s in shown.iter() {
                            div { key: "{s.id}", class: "tk-srow-wrap",
                                SourceRow {
                                    source: s.clone(),
                                    selected: s.picked || (!any_picked && s.recommended),
                                    on_pick: move |id: String| on_pick.call(id),
                                }
                                if let Some(handler) = on_download.filter(|_| s.kind == SourceKindVm::Torrent && s.ext_enabled) {
                                    IconButton {
                                        icon: "download".to_string(),
                                        label: download_label.clone(),
                                        variant: IconButtonVariant::Tonal,
                                        class: "tk-srow-wrap__dl".to_string(),
                                        onclick: {
                                            let sid = s.id.clone();
                                            move |_| handler.call(sid.clone())
                                        },
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

/// Details side panel: the picked (or recommended) source, "Chosen by the system" and "See all sources".
/// `on_play(id)` plays a source; `on_open_picker` asks the parent to open the `SourcePicker`.
#[component]
pub fn SourceSummary(media_id: String, episode: u32, on_play: EventHandler<String>, on_open_picker: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let mut sources = use_resource({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.sources(&id, episode).await }
        }
    });
    let mode = ui.settings.read().sources.mode;
    let mode_label = format!("{}: ", t("src.mode"));
    let mode_value = t(&format!("src.mode.{}", mode.id()));
    let count = match &*sources.read() {
        Some(Ok(list)) => list.len().to_string(),
        _ => String::new(),
    };
    let body = match &*sources.read() {
        None => rsx! {
            div { class: "tk-list", "aria-hidden": "true",
                div { class: "tk-skel tk-skel--line" }
                div { class: "tk-skel tk-skel--line", style: "width:60%" }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| sources.restart() }
        },
        Some(Ok(list)) => match list.iter().find(|s| s.picked).or(list.first()) {
            None => rsx! {
                EmptyState {
                    icon: "extensions".to_string(),
                    title: t("details.no_extension.title"),
                    body: t("details.no_extension.body"),
                    action_label: t("details.go_extensions"),
                    on_action: move |_| {
                        navigator.push(Route::ExtensionsPage {});
                    },
                }
            },
            Some(cur) => {
                let by_system = cur.recommended && !cur.picked;
                let all_label = tf("src.all", &[("n", &list.len().to_string())]);
                rsx! {
                    SourceRow { source: cur.clone(), selected: true, on_pick: move |id: String| on_play.call(id) }
                    if by_system {
                        p { class: "tk-muted tk-srcsum__by",
                            Icon { name: "check".to_string(), size: 16 }
                            {t("src.by_system")}
                        }
                    }
                    Button {
                        label: all_label,
                        variant: ButtonVariant::Tonal,
                        trailing_icon: "chevron-right".to_string(),
                        block: true,
                        onclick: move |_| on_open_picker.call(()),
                    }
                }
            }
        },
    };
    rsx! {
        div { class: "tk-panel tk-srcsum",
            h3 { class: "tk-panel__title",
                Icon { name: "extensions".to_string() }
                {t("src.title")}
                if !count.is_empty() {
                    span { class: "tk-srcsum__count", "{count}" }
                }
            }
            div { class: "tk-panel__row tk-panel__row--between",
                span { class: "tk-muted",
                    "{mode_label}"
                    b { "{mode_value}" }
                }
                Button {
                    label: t("settings.title"),
                    icon: "tune".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Xs,
                    onclick: move |_| {
                        navigator.push(Route::SettingsPage {});
                    },
                }
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliability_tones_follow_the_contract_thresholds() {
        assert_eq!(reliability_tone(0.95), "ok");
        assert_eq!(reliability_tone(0.9), "ok");
        assert_eq!(reliability_tone(0.75), "warn");
        assert_eq!(reliability_tone(0.5), "err");
        assert_eq!(reliability_key(0.5), "src.rel.low");
    }

    #[test]
    fn sizes_use_decimal_units() {
        assert_eq!(format_size(1_450_000_000), "1.4 GB");
        assert_eq!(format_size(350_000_000), "350 MB");
    }

    use crate::backend::ReleaseTagsVm;

    fn vm(label: &str, kind: SourceKindVm, quality: &str, reliability: f32, tags: ReleaseTagsVm) -> SourceVm {
        SourceVm {
            id: format!("ext|{}|{label}", kind.id()),
            ext_id: "ext".to_owned(),
            ext_name: "Ext".to_owned(),
            kind,
            label: label.to_owned(),
            quality: quality.to_owned(),
            container: None,
            subs: Vec::new(),
            dubs: Vec::new(),
            reliability,
            note: None,
            latency_ms: None,
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: false,
            picked: false,
            tags,
            debrid: None,
            embed: false,
        }
    }

    fn sample() -> Vec<SourceVm> {
        let bd = ReleaseTagsVm {
            group: Some("Judas".to_owned()),
            source: Some(ReleaseSourceVm::Bd),
            codec: Some(VideoCodecVm::Hevc),
            bit_depth: Some(10),
            dual_audio: true,
            multi_sub: true,
            batch: true,
            seadex_best: true,
            episodes: Some((1, 12)),
        };
        let web = ReleaseTagsVm { group: Some("SubsPlease".to_owned()), source: Some(ReleaseSourceVm::Web), episodes: Some((5, 5)), ..Default::default() };
        vec![
            vm("Judas", SourceKindVm::Torrent, "1080p", 0.95, bd),
            vm("Alpha", SourceKindVm::Web, "1080p", 0.8, ReleaseTagsVm::default()),
            vm("SubsPlease", SourceKindVm::Torrent, "720p", 0.6, web),
        ]
    }

    fn labels(list: &[SourceVm]) -> Vec<&str> {
        list.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn default_filters_keep_everything_in_order() {
        let list = sample();
        let f = SourceFilters::default();
        assert_eq!(labels(&f.apply(&list)), ["Judas", "Alpha", "SubsPlease"]);
        assert_eq!(f.active(), 0);
    }

    #[test]
    fn filters_combine_and_never_reorder() {
        let list = sample();
        let mut f = SourceFilters { kind: Some(SourceKindVm::Torrent), ..Default::default() };
        assert_eq!(labels(&f.apply(&list)), ["Judas", "SubsPlease"]);
        toggle(&mut f.sources, ReleaseSourceVm::Web);
        toggle(&mut f.sources, ReleaseSourceVm::Bd);
        assert_eq!(labels(&f.apply(&list)), ["Judas", "SubsPlease"]);
        toggle(&mut f.sources, ReleaseSourceVm::Web);
        assert_eq!(labels(&f.apply(&list)), ["Judas"]);
        assert_eq!(f.active(), 1);
        let mut f = SourceFilters::default();
        toggle(&mut f.resolutions, "1080p");
        assert_eq!(labels(&f.apply(&list)), ["Judas", "Alpha"]);
        f.ten_bit = true;
        assert_eq!(labels(&f.apply(&list)), ["Judas"]);
        f = SourceFilters { batch: true, seadex_best: true, dual_audio: true, multi_sub: true, ..Default::default() };
        assert_eq!(labels(&f.apply(&list)), ["Judas"]);
        assert_eq!(f.active(), 4);
        f = SourceFilters::default();
        toggle(&mut f.codecs, VideoCodecVm::Av1);
        assert!(f.apply(&list).is_empty());
    }

    #[test]
    fn reliability_floor_toggles() {
        let list = sample();
        let mut f = SourceFilters::default();
        f.toggle_floor(RelFloor::Mid);
        assert_eq!(labels(&f.apply(&list)), ["Judas", "Alpha"]);
        f.toggle_floor(RelFloor::High);
        assert_eq!(labels(&f.apply(&list)), ["Judas"]);
        f.toggle_floor(RelFloor::High);
        assert_eq!(f.min_reliability, None);
        assert_eq!(f.apply(&list).len(), 3);
    }

    #[test]
    fn options_only_offer_what_the_list_has() {
        let o = FilterOptions::of(&sample());
        assert_eq!(o.sources, [ReleaseSourceVm::Bd, ReleaseSourceVm::Web]);
        assert_eq!(o.resolutions, ["1080p", "720p"]);
        assert_eq!(o.codecs, [VideoCodecVm::Hevc]);
        assert!(o.ten_bit && o.dual_audio && o.multi_sub && o.batch && o.seadex_best && o.any_feature());
        assert_eq!(FilterOptions::of(&[]), FilterOptions::default());
        assert!(!FilterOptions::default().any_feature());
    }

    #[test]
    fn tag_chips_follow_the_tags() {
        let list = sample();
        let chips = tag_chips(&list[0]);
        let tones: Vec<&str> = chips.iter().map(|c| c.1).collect();
        // SeaDex best, (group == label: no chip), BD, 1080p, HEVC, 10-bit, dual audio, multi-sub, batch
        assert_eq!(tones, ["best", "", "", "", "", "", "", "batch"]);
        assert_eq!(chips[1].0, "BD");
        assert_eq!(chips[2].0, "1080p");
        assert_eq!(chips[3].0, "HEVC");
        assert!(chips[7].0.contains("12"));
        // a group that differs from the label gets its own chip; web rows without tags get none
        let mut other = list[2].clone();
        other.label = "SubsPlease 720p".to_owned();
        assert_eq!(tag_chips(&other)[0], ("SubsPlease".to_owned(), "group"));
        assert!(tag_chips(&list[1]).is_empty());
    }
}
