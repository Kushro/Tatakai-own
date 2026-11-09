//! Typography and reading-flow controls of the novel reader (worker **m**; checklist `novel.reader.typography`,
//! `novel.reader.custom-font`, `novel.reader.flow`, `novel.reader.settings-sheet`; classic `NovelReaderPanel.tsx`).
//! The same controls appear in the reader's in-place settings sheet and in Settings > Novel reader; both commit
//! `SettingsAction::SetNovel` at once (the reader re-renders live, `data-font-size` changes).

use dioxus::prelude::*;

use crate::app::Ui;
use crate::backend::BackendRef;
use crate::components::{Segmented, Switch};
use crate::i18n::t;
use crate::model::novel::{
    FAMILY_PREFIX, FONT_SIZE_MAX, FONT_SIZE_MIN, LINE_HEIGHT_MAX, LINE_HEIGHT_MIN, MAX_WIDTH_MAX, MAX_WIDTH_MIN,
    NovelFlowPrefs, PARAGRAPH_GAP_MAX, READER_FONTS,
};
use crate::model::{NovelPrefs, NovelThemeVm, SettingsAction, reduce_settings};
use crate::screens::player::subs::{app_fonts_dir, use_font_families};

/// Persist new novel prefs (sanitized by the reducer).
pub fn commit_novel(ui: Ui, backend: &BackendRef, next: NovelPrefs) {
    let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetNovel(next));
    ui.commit_settings(backend, s);
}

/// Select value of the font field: a built-in id, or `family` for an installed font.
pub fn font_choice(font: &str) -> &str {
    if font.starts_with(FAMILY_PREFIX) { "family" } else { font }
}

/// `prefs` with a numeric field parsed from a range input (`None` when not a number).
pub fn with_number(prefs: &NovelPrefs, field: &str, value: &str) -> Option<NovelPrefs> {
    let n: u32 = value.trim().parse().ok()?;
    let mut next = prefs.clone();
    match field {
        "font_size" => next.font_size = n.min(u32::from(u8::MAX)) as u8,
        "line_height" => next.line_height = n.min(u32::from(u8::MAX)) as u8,
        "paragraph_gap" => next.paragraph_gap = n.min(u32::from(u8::MAX)) as u8,
        "max_width" => next.max_width = n.min(u32::from(u16::MAX)) as u16,
        _ => return None,
    }
    Some(next)
}

/// Font, size, line height, paragraph gap, width, theme, justify, and the custom font file.
#[component]
pub fn TypographyControls(prefs: NovelPrefs, data_dir: String, on_change: EventHandler<NovelPrefs>) -> Element {
    let (families, _rescan) = use_font_families(app_fonts_dir(&data_dir));
    let families_list = families.read().clone().unwrap_or_default();
    let choice = font_choice(&prefs.font).to_owned();
    let family_now = prefs.font.strip_prefix(FAMILY_PREFIX).unwrap_or_default().to_owned();
    let theme_opts: Vec<(String, String)> =
        NovelThemeVm::ALL.iter().map(|th| (th.id().to_owned(), t(&format!("novel.theme.{}", th.id())))).collect();
    let custom_name = prefs
        .custom_font_path
        .as_deref()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let number = {
        let prefs = prefs.clone();
        move |field: &'static str| {
            let prefs = prefs.clone();
            move |e: FormEvent| {
                if let Some(next) = with_number(&prefs, field, &e.value()) {
                    on_change.call(next);
                }
            }
        }
    };
    let p_font = prefs.clone();
    let p_family = prefs.clone();
    let p_file = prefs.clone();
    let p_theme = prefs.clone();
    let p_justify = prefs.clone();
    let gap = format!("{:.1}", f32::from(prefs.paragraph_gap) / 10.0);
    rsx! {
        div { class: "tk-novel-ctl", "data-font": "{prefs.font}", "data-font-size": "{prefs.font_size}",
            label { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.font")} }
                select {
                    "aria-label": t("novel.font"),
                    value: "{choice}",
                    onchange: move |e| {
                        let v = e.value();
                        let mut next = p_font.clone();
                        next.font = if v == "family" {
                            format!("{FAMILY_PREFIX}{}", families_first(&p_font))
                        } else {
                            v
                        };
                        on_change.call(next);
                    },
                    for f in READER_FONTS {
                        option { key: "{f}", value: "{f}", selected: choice == f, {t(&format!("novel.font.{f}"))} }
                    }
                    option { value: "family", selected: choice == "family", {t("novel.font.family")} }
                }
            }
            if choice == "family" {
                label { class: "tk-novel-ctl__row",
                    span { class: "tk-novel-ctl__label", {t("novel.font.installed")} }
                    select {
                        "aria-label": t("novel.font.installed"),
                        value: "{family_now}",
                        onchange: move |e| {
                            let mut next = p_family.clone();
                            next.font = format!("{FAMILY_PREFIX}{}", e.value());
                            on_change.call(next);
                        },
                        if families_list.is_empty() {
                            option { value: "{family_now}", selected: true, "{family_now}" }
                        }
                        for fam in families_list.clone() {
                            option { key: "{fam}", value: "{fam}", selected: fam == family_now, "{fam}" }
                        }
                    }
                }
            }
            if choice == "custom" {
                div { class: "tk-novel-ctl__row", "data-custom-font": if custom_name.is_empty() { "none" } else { "set" },
                    span { class: "tk-novel-ctl__label", {t("novel.font.custom")} }
                    label { class: "tk-filebtn tk-btn tk-btn--tonal tk-btn--sm",
                        span { {t("novel.font.pick")} }
                        input {
                            r#type: "file",
                            accept: ".ttf,.otf,.woff,.woff2",
                            onchange: move |e| {
                                if let Some(f) = e.files().first() {
                                    let mut next = p_file.clone();
                                    next.custom_font_path = Some(f.path().to_string_lossy().into_owned());
                                    next.font = "custom".into();
                                    on_change.call(next);
                                }
                            },
                        }
                    }
                    if custom_name.is_empty() {
                        span { class: "tk-muted", {t("novel.font.custom_none")} }
                    } else {
                        code { class: "tk-novel-ctl__file", "{custom_name}" }
                    }
                }
            }
            label { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.font_size")} }
                input {
                    r#type: "range",
                    min: "{FONT_SIZE_MIN}",
                    max: "{FONT_SIZE_MAX}",
                    step: "1",
                    value: "{prefs.font_size}",
                    "aria-label": t("novel.font_size"),
                    "data-control": "font-size",
                    oninput: number("font_size"),
                }
                span { class: "tk-novel-ctl__value", "{prefs.font_size} px" }
            }
            label { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.line_height")} }
                input {
                    r#type: "range",
                    min: "{LINE_HEIGHT_MIN}",
                    max: "{LINE_HEIGHT_MAX}",
                    step: "5",
                    value: "{prefs.line_height}",
                    "aria-label": t("novel.line_height"),
                    oninput: number("line_height"),
                }
                span { class: "tk-novel-ctl__value", "{prefs.line_height} %" }
            }
            label { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.paragraph_gap")} }
                input {
                    r#type: "range",
                    min: "0",
                    max: "{PARAGRAPH_GAP_MAX}",
                    step: "1",
                    value: "{prefs.paragraph_gap}",
                    "aria-label": t("novel.paragraph_gap"),
                    oninput: number("paragraph_gap"),
                }
                span { class: "tk-novel-ctl__value", "{gap} em" }
            }
            label { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.max_width")} }
                input {
                    r#type: "range",
                    min: "{MAX_WIDTH_MIN}",
                    max: "{MAX_WIDTH_MAX}",
                    step: "20",
                    value: "{prefs.max_width}",
                    "aria-label": t("novel.max_width"),
                    oninput: number("max_width"),
                }
                span { class: "tk-novel-ctl__value", "{prefs.max_width} px" }
            }
            div { class: "tk-novel-ctl__row", "data-theme-choice": "{prefs.theme.id()}",
                span { class: "tk-novel-ctl__label", {t("novel.theme")} }
                Segmented {
                    options: theme_opts,
                    selected: prefs.theme.id().to_string(),
                    label: t("novel.theme"),
                    small: true,
                    on_select: move |id: String| {
                        let mut next = p_theme.clone();
                        next.theme = NovelThemeVm::ALL.into_iter().find(|th| th.id() == id).unwrap_or_default();
                        on_change.call(next);
                    },
                }
            }
            div { class: "tk-novel-ctl__row",
                span { class: "tk-novel-ctl__label", {t("novel.justify")} }
                Switch {
                    checked: prefs.justify,
                    label: t("novel.justify"),
                    on_toggle: move |v: bool| {
                        let mut next = p_justify.clone();
                        next.justify = v;
                        on_change.call(next);
                    },
                }
            }
        }
    }
}

/// Installed family to start from when "Installed font" is picked: the current one, else a common serif.
fn families_first(prefs: &NovelPrefs) -> String {
    prefs.font.strip_prefix(FAMILY_PREFIX).map(str::to_owned).unwrap_or_else(|| "Noto Serif".to_owned())
}

/// Continuous scroll, progress bar, percent, chapter number, keep the screen on.
#[component]
pub fn FlowControls(prefs: NovelPrefs, #[props(default)] wake_supported: Option<bool>, on_change: EventHandler<NovelPrefs>) -> Element {
    let row = |key: &'static str, hint: &'static str, on: bool, set: fn(&mut NovelFlowPrefs, bool)| {
        let base = prefs.clone();
        rsx! {
            div { class: "tk-novel-ctl__row", "data-flow": "{key}",
                div { class: "tk-novel-ctl__text",
                    span { class: "tk-novel-ctl__label", {t(key)} }
                    if !hint.is_empty() {
                        span { class: "tk-novel-ctl__hint tk-muted", {t(hint)} }
                    }
                }
                Switch {
                    checked: on,
                    label: t(key),
                    on_toggle: move |v: bool| {
                        let mut next = base.clone();
                        set(&mut next.flow, v);
                        on_change.call(next);
                    },
                }
            }
        }
    };
    let f = prefs.flow.clone();
    let wake_attr = match wake_supported {
        Some(true) => "supported",
        Some(false) => "unsupported",
        None => "unknown",
    };
    rsx! {
        div { class: "tk-novel-ctl", "data-wake-lock": "{wake_attr}",
            {row("novel.flow.continuous", "novel.flow.continuous_hint", f.continuous, |fl, v| fl.continuous = v)}
            {row("novel.flow.progress", "", f.show_progress, |fl, v| fl.show_progress = v)}
            {row("novel.flow.percent", "", f.show_percent, |fl, v| fl.show_percent = v)}
            {row("novel.flow.chapter", "", f.show_chapter, |fl, v| fl.show_chapter = v)}
            {row("novel.flow.keep_awake", "novel.flow.keep_awake_hint", f.keep_awake, |fl, v| fl.keep_awake = v)}
            if wake_supported == Some(false) && f.keep_awake {
                p { class: "tk-novel-ctl__hint tk-muted", role: "status", {t("novel.flow.keep_awake_unsupported")} }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_fields() {
        let p = NovelPrefs::default();
        assert_eq!(with_number(&p, "font_size", "22").map(|n| n.font_size), Some(22));
        assert_eq!(with_number(&p, "max_width", "900").map(|n| n.max_width), Some(900));
        assert_eq!(with_number(&p, "line_height", "x"), None);
        assert_eq!(with_number(&p, "nope", "3"), None);
        assert_eq!(font_choice("family:Lora"), "family");
        assert_eq!(font_choice("sans"), "sans");
    }
}
