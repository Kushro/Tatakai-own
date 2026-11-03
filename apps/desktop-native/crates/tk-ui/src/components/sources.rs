//! `sources.rs`: SourceRow, SourcePicker (sheet) and SourceSummary (Details side panel), docs/redesign/07
//! section 3.4 and the prototype `sources.js`. Props are the wave 1 contract (docs/redesign/08 section 6).
//!
//! Unlike the other components, `SourceSummary` loads its own data: it reads the backend from the
//! `Services` context (`use_context::<Services>().backend`, a `BackendRef`) and the prefs from the `Ui`
//! context (`ui.settings.read().sources`). Its resource does not track prop changes, so callers key it
//! per episode: `SourceSummary { key: "{id}-{ep}", .. }`.

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{SourceKindVm, SourceVm};
use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::components::chips::{Banner, InfoChip, Pill, Tone};
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
            out.push((t("src.group"), s.label.clone()));
            if !quality.is_empty() {
                out.push((t("player.quality"), quality));
            }
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
    let pct = (source.reliability.clamp(0.0, 1.0) * 100.0).round() as u32;
    let tone = reliability_tone(source.reliability);
    let rel_text = format!("{}: {} · {pct} %", t("src.reliability"), t(reliability_key(source.reliability)));
    rsx! {
        button {
            class: "tk-srow",
            r#type: "button",
            role: "radio",
            "aria-checked": "{selected}",
            "data-kind": "{kind}",
            disabled,
            onclick: move |_| on_pick.call(id.clone()),
            span { class: "tk-srow__ico", Icon { name: icon.to_string(), size: 20 } }
            span { class: "tk-srow__main",
                span { class: "tk-srow__title",
                    b { "{name}" }
                    Pill { text: kind_text, tone: Tone::Neutral, plain: true }
                    if source.recommended {
                        Pill { text: t("src.recommended"), tone: Tone::Primary }
                    }
                    if disabled {
                        Pill { text: off_text, tone: Tone::Err, plain: true }
                    }
                    if let Some(note) = source.note.clone() {
                        span { class: "tk-srow__note", "{note}" }
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
#[component]
pub fn SourcePicker(
    open: bool,
    sources: Vec<SourceVm>,
    prefs: SourcePrefsVm,
    on_pick: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut filter = use_signal(|| "all".to_string());
    let navigator = use_navigator();
    if !open {
        return rsx! {};
    }
    let kind = filter();
    let any_picked = sources.iter().any(|s| s.picked);
    let mut ranked = sources.clone();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    let shown: Vec<SourceVm> = ranked.into_iter().filter(|s| kind == "all" || s.kind.id() == kind).collect();
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
                    on_select: move |k: String| filter.set(k),
                }
                div { class: "tk-chips",
                    InfoChip { text: sub_chip, icon: "cc".to_string() }
                    InfoChip { text: dub_chip, icon: "audio".to_string() }
                    InfoChip { text: quality_chip, icon: "hd".to_string() }
                    if let Some(p) = prefer_chip {
                        InfoChip { text: p, icon: "filter".to_string() }
                    }
                }
                if shown.is_empty() {
                    p { class: "tk-muted", {t("src.none")} }
                } else {
                    div { class: "tk-slist", role: "radiogroup", "aria-label": "{list_label}",
                        for s in shown.iter() {
                            SourceRow {
                                key: "{s.id}",
                                source: s.clone(),
                                selected: s.picked || (!any_picked && s.recommended),
                                on_pick: move |id: String| on_pick.call(id),
                            }
                        }
                    }
                }
                p { class: "tk-muted tk-spicker__note", {t("src.m2")} }
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
}
