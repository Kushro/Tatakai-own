//! Every `MangaReaderPrefs` option as a form (worker **l**; checklist `manga.reader.modes`, `manga.reader.fit`,
//! `manga.reader.loading`, `manga.reader.behavior`; classic `panels/ReaderPanel.tsx`). Used by the reader's settings
//! sheet (per session, with "Save as default") and by Settings > Manga reader (`settings/reader.rs`). Pure props in,
//! the whole struct out through `on_change` (the caller sanitizes / commits).

use dioxus::prelude::*;

use crate::components::{SegOption, Segmented, Switch};
use crate::i18n::{t, tf};
use crate::model::manga::{
    AUTO_SCROLL_MAX, AUTO_SCROLL_MIN, BRIGHTNESS_MAX, BRIGHTNESS_MIN, GAP_MAX_PX, MAX_WIDTH_PX_MAX,
    PREFETCH_MAX_PAGES, WIDTH_MAX_PERCENT, WIDTH_MIN_PERCENT, ZOOM_MAX_PERCENT, ZOOM_MIN_PERCENT,
};
use crate::model::{MangaReaderPrefs, ReaderBackgroundVm, ReaderDirectionVm, ReaderFitVm, ReaderModeVm};

fn opts<I: Copy>(items: &[I], id: impl Fn(I) -> &'static str, key: &str) -> Vec<SegOption> {
    items
        .iter()
        .map(|i| (id(*i).to_owned(), t(&format!("{key}.{}", id(*i)))))
        .collect()
}

/// One labelled row of the form.
#[component]
fn FormRow(
    label: String,
    #[props(default)] hint: String,
    #[props(default)] value: String,
    children: Element,
) -> Element {
    rsx! {
        div { class: "tk-manga-form__row",
            div { class: "tk-manga-form__text",
                span { class: "tk-manga-form__label", "{label}" }
                if !hint.is_empty() {
                    span { class: "tk-manga-form__hint tk-muted", "{hint}" }
                }
            }
            div { class: "tk-manga-form__ctl",
                {children}
                if !value.is_empty() {
                    span { class: "tk-manga-form__value", "{value}" }
                }
            }
        }
    }
}

/// A numeric slider row.
#[component]
fn RangeRow(
    label: String,
    #[props(default)] hint: String,
    min: u32,
    max: u32,
    #[props(default = 1)] step: u32,
    value: u32,
    shown: String,
    attr: String,
    on_input: EventHandler<u32>,
) -> Element {
    rsx! {
        FormRow { label: label.clone(), hint, value: shown,
            input {
                class: "tk-manga-form__range",
                r#type: "range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                "aria-label": "{label}",
                "data-pref": "{attr}",
                oninput: move |e| {
                    if let Ok(v) = e.value().parse::<u32>() {
                        on_input.call(v.clamp(min, max));
                    }
                },
            }
        }
    }
}

#[component]
pub fn ReaderPrefsForm(
    prefs: MangaReaderPrefs,
    on_change: EventHandler<MangaReaderPrefs>,
    /// Hide the behaviour block (the reader sheet shows it; Settings too).
    #[props(default)]
    compact: bool,
) -> Element {
    let p = prefs.clone();
    // Always up to date (`use_callback` replaces the closure every render) and `Copy`, so every control can own it.
    let set = use_callback(move |f: Box<dyn Fn(&mut MangaReaderPrefs)>| {
        let mut next = prefs.clone();
        f(&mut next);
        on_change.call(next.sanitized());
    });
    let vertical = matches!(p.mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon);
    let max_width = if p.max_width_px == 0 {
        t("manga.reader.opt.no_limit")
    } else {
        format!("{} px", p.max_width_px)
    };
    rsx! {
        div { class: "tk-manga-form", "data-reader-mode": "{p.mode.id()}", "data-reader-fit": "{p.fit.id()}",
            h4 { class: "tk-manga-form__group", {t("manga.reader.group.layout")} }
            FormRow { label: t("manga.reader.opt.mode"),
                Segmented {
                    options: opts(&ReaderModeVm::ALL, ReaderModeVm::id, "manga.reader.mode"),
                    selected: p.mode.id().to_owned(),
                    label: t("manga.reader.opt.mode"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(m) = ReaderModeVm::ALL.into_iter().find(|m| m.id() == id) {
                            set.call(Box::new(move |x: &mut MangaReaderPrefs| x.mode = m));
                        }
                    },
                }
            }
            FormRow { label: t("manga.reader.opt.direction"), hint: t("manga.reader.opt.direction_hint"),
                Segmented {
                    options: opts(&ReaderDirectionVm::ALL, ReaderDirectionVm::id, "manga.reader.direction"),
                    selected: p.direction.id().to_owned(),
                    label: t("manga.reader.opt.direction"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(d) = ReaderDirectionVm::ALL.into_iter().find(|d| d.id() == id) {
                            set.call(Box::new(move |x: &mut MangaReaderPrefs| x.direction = d));
                        }
                    },
                }
            }
            FormRow { label: t("manga.reader.opt.fit"),
                Segmented {
                    options: opts(&ReaderFitVm::ALL, ReaderFitVm::id, "manga.reader.fit"),
                    selected: p.fit.id().to_owned(),
                    label: t("manga.reader.opt.fit"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(f) = ReaderFitVm::ALL.into_iter().find(|f| f.id() == id) {
                            set.call(Box::new(move |x: &mut MangaReaderPrefs| x.fit = f));
                        }
                    },
                }
            }
            if p.fit == ReaderFitVm::Original {
                RangeRow {
                    label: t("manga.reader.opt.zoom"),
                    min: u32::from(ZOOM_MIN_PERCENT),
                    max: u32::from(ZOOM_MAX_PERCENT),
                    step: 10,
                    value: u32::from(p.zoom_percent),
                    shown: format!("{} %", p.zoom_percent),
                    attr: "zoom",
                    on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.zoom_percent = v as u16)),
                }
            }
            if vertical {
                RangeRow {
                    label: t("manga.reader.opt.width"),
                    min: u32::from(WIDTH_MIN_PERCENT),
                    max: u32::from(WIDTH_MAX_PERCENT),
                    value: u32::from(p.width_percent),
                    shown: format!("{} %", p.width_percent),
                    attr: "width",
                    on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.width_percent = v as u8)),
                }
            }
            RangeRow {
                label: t("manga.reader.opt.max_width"),
                hint: t("manga.reader.opt.max_width_hint"),
                min: 0,
                max: u32::from(MAX_WIDTH_PX_MAX),
                step: 100,
                value: u32::from(p.max_width_px),
                shown: max_width,
                attr: "max-width",
                on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.max_width_px = v as u16)),
            }
            if p.mode == ReaderModeVm::Vertical {
                RangeRow {
                    label: t("manga.reader.opt.gap"),
                    min: 0,
                    max: u32::from(GAP_MAX_PX),
                    value: u32::from(p.gap_px),
                    shown: format!("{} px", p.gap_px),
                    attr: "gap",
                    on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.gap_px = v as u8)),
                }
            }
            FormRow { label: t("manga.reader.opt.background"),
                Segmented {
                    options: opts(&ReaderBackgroundVm::ALL, ReaderBackgroundVm::id, "manga.reader.background"),
                    selected: p.background.id().to_owned(),
                    label: t("manga.reader.opt.background"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(b) = ReaderBackgroundVm::ALL.into_iter().find(|b| b.id() == id) {
                            set.call(Box::new(move |x: &mut MangaReaderPrefs| x.background = b));
                        }
                    },
                }
            }
            RangeRow {
                label: t("manga.reader.opt.brightness"),
                min: u32::from(BRIGHTNESS_MIN),
                max: u32::from(BRIGHTNESS_MAX),
                value: u32::from(p.brightness),
                shown: format!("{} %", p.brightness),
                attr: "brightness",
                on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.brightness = v as u8)),
            }
            FormRow { label: t("manga.reader.opt.panel_crop"), hint: t("manga.reader.opt.panel_crop_hint"),
                Switch { checked: p.panel_crop, label: t("manga.reader.opt.panel_crop"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.panel_crop = v)) }
            }

            h4 { class: "tk-manga-form__group", {t("manga.reader.group.loading")} }
            FormRow { label: t("manga.reader.opt.eager"), hint: t("manga.reader.opt.eager_hint"),
                Switch { checked: p.eager_loading, label: t("manga.reader.opt.eager"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.eager_loading = v)) }
            }
            RangeRow {
                label: t("manga.reader.opt.prefetch"),
                min: 0,
                max: u32::from(PREFETCH_MAX_PAGES),
                value: u32::from(p.prefetch_pages),
                shown: tf("manga.reader.opt.pages_n", &[("n", &p.prefetch_pages.to_string())]),
                attr: "prefetch",
                on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.prefetch_pages = v as u8)),
            }
            FormRow { label: t("manga.reader.opt.prefetch_next"), hint: t("manga.reader.opt.prefetch_next_hint"),
                Switch {
                    checked: p.prefetch_next_chapter,
                    label: t("manga.reader.opt.prefetch_next"),
                    on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.prefetch_next_chapter = v)),
                }
            }
            FormRow { label: t("manga.reader.opt.continuous"), hint: t("manga.reader.opt.continuous_hint"),
                Switch { checked: p.continuous_scroll, label: t("manga.reader.opt.continuous"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.continuous_scroll = v)) }
            }

            if !compact {
                h4 { class: "tk-manga-form__group", {t("manga.reader.group.behavior")} }
                RangeRow {
                    label: t("manga.reader.opt.auto_scroll"),
                    hint: t("manga.reader.opt.auto_scroll_hint"),
                    min: u32::from(AUTO_SCROLL_MIN),
                    max: u32::from(AUTO_SCROLL_MAX),
                    value: u32::from(p.auto_scroll_speed),
                    shown: p.auto_scroll_speed.to_string(),
                    attr: "auto-scroll",
                    on_input: move |v: u32| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.auto_scroll_speed = v as u8)),
                }
                FormRow { label: t("manga.reader.opt.tap_zones"), hint: t("manga.reader.opt.tap_zones_hint"),
                    Switch { checked: p.tap_zones, label: t("manga.reader.opt.tap_zones"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.tap_zones = v)) }
                }
                FormRow { label: t("manga.reader.opt.zoom_wheel"),
                    Switch { checked: p.zoom_with_wheel, label: t("manga.reader.opt.zoom_wheel"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.zoom_with_wheel = v)) }
                }
                FormRow { label: t("manga.reader.opt.auto_fullscreen"),
                    Switch { checked: p.auto_fullscreen, label: t("manga.reader.opt.auto_fullscreen"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.auto_fullscreen = v)) }
                }
                FormRow { label: t("manga.reader.opt.hide_chrome"), hint: t("manga.reader.opt.hide_chrome_hint"),
                    Switch {
                        checked: p.hide_chrome_fullscreen,
                        label: t("manga.reader.opt.hide_chrome"),
                        on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.hide_chrome_fullscreen = v)),
                    }
                }
                FormRow { label: t("manga.reader.opt.page_number"),
                    Switch { checked: p.show_page_number, label: t("manga.reader.opt.page_number"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.show_page_number = v)) }
                }
                FormRow { label: t("manga.reader.opt.progress_bar"),
                    Switch { checked: p.show_progress_bar, label: t("manga.reader.opt.progress_bar"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.show_progress_bar = v)) }
                }
                FormRow { label: t("manga.reader.opt.capture"), hint: t("manga.reader.opt.capture_hint"),
                    Switch { checked: p.show_capture_button, label: t("manga.reader.opt.capture"), on_toggle: move |v: bool| set.call(Box::new(move |x: &mut MangaReaderPrefs| x.show_capture_button = v)) }
                }
            }
        }
    }
}
