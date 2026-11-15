//! Settings > Appearance: palette, mode, contrast, language, title language (wave A, worker c), reduce motion.
//! Wave B (worker **b**, `settings.appearance.more-palettes`): the 20 palettes of `packages/tokens/tokens.json`
//! (`model::home::ALL_PALETTES`, which `model::PALETTES` mirrors), one swatch each with `data-palette`.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: nobody in wave A (integrator). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::{SettingRow, seg};
use crate::app::Ui;
use crate::components::{Segmented, Switch};
use crate::i18n::{LangChoice, t, tf};
use crate::model::home::ALL_PALETTES;
use crate::model::look::{HERO_INTERVAL_MAX, HERO_INTERVAL_MIN};
use crate::model::{
    CalendarView, CardStyle, Contrast, Density, EpisodeView, HomeLayout, LookPrefs, LookPreset, Motion, RailStyle,
    RelationsView, SettingsAction, TITLE_LANGUAGES, ThemeMode,
};

/// `(id, label)` options of a look choice (`look.<group>.<id>` labels).
fn look_seg<I: Copy>(items: &[I], id: impl Fn(I) -> &'static str, key: impl Fn(I) -> String) -> Vec<(String, String)> {
    items.iter().map(|i| (id(*i).to_owned(), t(&key(*i)))).collect()
}

#[component]
pub fn AppearanceSection(
    commit: Callback<SettingsAction>,
    commit_saved: Callback<SettingsAction>,
) -> Element {
    let ui = use_context::<Ui>();
    let s = (ui.settings)();
    rsx! {
        SettingRow {
            label: t("settings.palette"),
            hint: format!("{} · {}", t("settings.palette_hint"), tf("settings.appearance.palettes_count", &[("n", &ALL_PALETTES.len().to_string())])),
            stack: true,
            div { class: "tk-swatches", role: "radiogroup", "aria-label": t("settings.palette"), "data-palettes": "{ALL_PALETTES.len()}",
                for p in ALL_PALETTES {
                    {
                        let label = t(&format!("palette.{}", p.id));
                        let selected = s.palette == p.id;
                        rsx! {
                            button {
                                key: "{p.id}",
                                class: "tk-swatch",
                                r#type: "button",
                                role: "radio",
                                "aria-checked": "{selected}",
                                "aria-label": "{label}",
                                title: "{label}",
                                "data-selected": "{selected}",
                                "data-palette": "{p.id}",
                                onclick: move |_| commit.call(SettingsAction::SetPalette(p.id.to_owned())),
                                span { class: "tk-swatch__dot", style: "background-color:{p.seed}" }
                                span { class: "tk-swatch__name", "{label}" }
                            }
                        }
                    }
                }
            }
        }
        SettingRow { label: t("settings.mode"),
            Segmented {
                options: seg(&ThemeMode::ALL, ThemeMode::id, "mode"),
                selected: s.mode.id().to_owned(),
                label: t("settings.mode"),
                on_select: move |id: String| {
                    if let Some(m) = ThemeMode::ALL.into_iter().find(|m| m.id() == id) {
                        commit.call(SettingsAction::SetMode(m));
                    }
                },
            }
        }
        SettingRow { label: t("settings.contrast"),
            Segmented {
                options: seg(&Contrast::ALL, Contrast::id, "contrast"),
                selected: s.contrast.id().to_owned(),
                label: t("settings.contrast"),
                on_select: move |id: String| {
                    if let Some(c) = Contrast::ALL.into_iter().find(|c| c.id() == id) {
                        commit.call(SettingsAction::SetContrast(c));
                    }
                },
            }
        }
        SettingRow { label: t("settings.language"),
            Segmented {
                options: seg(&LangChoice::ALL, LangChoice::id, "lang"),
                selected: s.language.id().to_owned(),
                label: t("settings.language"),
                on_select: move |id: String| {
                    if let Some(l) = LangChoice::ALL.into_iter().find(|l| l.id() == id) {
                        commit.call(SettingsAction::SetLanguage(l));
                    }
                },
            }
        }
        SettingRow { label: t("details.title_lang"), hint: t("details.title_lang.hint"),
            Segmented {
                options: seg(&TITLE_LANGUAGES, |l| l, "details.title_lang"),
                selected: s.title_language.clone(),
                label: t("details.title_lang"),
                on_select: move |id: String| commit.call(SettingsAction::SetTitleLanguage(id)),
            }
        }
        LookSection { commit }
        SettingRow { label: t("settings.reduce_motion"), hint: t("settings.reduce_motion_hint"),
            Switch {
                checked: s.reduce_motion,
                label: t("settings.reduce_motion"),
                on_toggle: move |v: bool| commit_saved.call(SettingsAction::SetReduceMotion(v)),
            }
        }
    }
}

/// "Layout and style" (2026-11 UI refresh): presets and every `Settings::look` choice. Changes apply live.
#[component]
fn LookSection(commit: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let look = ui.settings.read().look.clone();
    let matching = look.matching_preset();
    let set = move |next: LookPrefs| commit.call(SettingsAction::SetLook(next));
    // A field setter that keeps the rest of the current prefs.
    let with = move |f: &dyn Fn(&mut LookPrefs)| {
        let mut next = ui.settings.peek().look.clone();
        f(&mut next);
        set(next);
    };
    let preset_hint = match matching {
        Some(_) => t("look.preset.hint"),
        None => format!("{} · {}", t("look.preset.custom"), t("look.preset.hint")),
    };
    rsx! {
        h3 { class: "tk-settings__subhead", "data-look": "section", {t("look.section")} }
        SettingRow { label: t("look.preset"), hint: preset_hint, stack: true,
            div { class: "tk-presets", role: "radiogroup", "aria-label": t("look.preset"),
                for p in LookPreset::ALL.iter().copied() {
                    {
                        let selected = matching == Some(p);
                        let label = t(&p.label_key());
                        let hint = t(&format!("{}.hint", p.label_key()));
                        rsx! {
                            button {
                                key: "{p.id()}",
                                class: "tk-preset",
                                r#type: "button",
                                role: "radio",
                                "aria-checked": "{selected}",
                                "data-preset": "{p.id()}",
                                onclick: move |_| set(LookPrefs::preset(p)),
                                span { class: "tk-preset__thumb", "aria-hidden": "true", "data-preset": "{p.id()}",
                                    span { class: "tk-preset__rail" }
                                    span { class: "tk-preset__body",
                                        span { class: "tk-preset__hero" }
                                        span { class: "tk-preset__cards", i {} i {} i {} i {} i {} }
                                        span { class: "tk-preset__cards", i {} i {} i {} i {} i {} }
                                    }
                                }
                                span { class: "tk-preset__name", "{label}" }
                                span { class: "tk-preset__hint", "{hint}" }
                            }
                        }
                    }
                }
            }
        }
        SettingRow { label: t("look.home"), hint: t("look.home.hint"),
            Segmented {
                options: look_seg(HomeLayout::ALL, HomeLayout::id, HomeLayout::label_key),
                selected: look.home_layout.id().to_owned(),
                label: t("look.home"),
                on_select: move |id: String| if let Some(v) = HomeLayout::from_id(&id) { with(&|l| l.home_layout = v) },
            }
        }
        SettingRow { label: t("look.cards"), hint: t("look.cards.hint"),
            Segmented {
                options: look_seg(CardStyle::ALL, CardStyle::id, CardStyle::label_key),
                selected: look.card_style.id().to_owned(),
                label: t("look.cards"),
                on_select: move |id: String| if let Some(v) = CardStyle::from_id(&id) { with(&|l| l.card_style = v) },
            }
        }
        SettingRow { label: t("look.rail"), hint: t("look.rail.hint"),
            Segmented {
                options: look_seg(RailStyle::ALL, RailStyle::id, RailStyle::label_key),
                selected: look.rail_style.id().to_owned(),
                label: t("look.rail"),
                on_select: move |id: String| if let Some(v) = RailStyle::from_id(&id) { with(&|l| l.rail_style = v) },
            }
        }
        SettingRow { label: t("look.density"),
            Segmented {
                options: look_seg(Density::ALL, Density::id, Density::label_key),
                selected: look.density.id().to_owned(),
                label: t("look.density"),
                on_select: move |id: String| if let Some(v) = Density::from_id(&id) { with(&|l| l.density = v) },
            }
        }
        SettingRow { label: t("look.motion"), hint: t("look.motion.hint"),
            Segmented {
                options: look_seg(Motion::ALL, Motion::id, Motion::label_key),
                selected: look.motion.id().to_owned(),
                label: t("look.motion"),
                on_select: move |id: String| if let Some(v) = Motion::from_id(&id) { with(&|l| l.motion = v) },
            }
        }
        SettingRow { label: t("look.glass"), hint: t("look.glass.hint"),
            Switch {
                checked: look.glass,
                label: t("look.glass"),
                on_toggle: move |v: bool| with(&|l| l.glass = v),
            }
        }
        SettingRow { label: t("look.episodes"), hint: t("look.episodes.hint"),
            Segmented {
                options: look_seg(EpisodeView::ALL, EpisodeView::id, EpisodeView::label_key),
                selected: look.episode_view.id().to_owned(),
                label: t("look.episodes"),
                on_select: move |id: String| if let Some(v) = EpisodeView::from_id(&id) { with(&|l| l.episode_view = v) },
            }
        }
        SettingRow { label: t("look.relations"),
            Segmented {
                options: look_seg(RelationsView::ALL, RelationsView::id, RelationsView::label_key),
                selected: look.relations_view.id().to_owned(),
                label: t("look.relations"),
                on_select: move |id: String| if let Some(v) = RelationsView::from_id(&id) { with(&|l| l.relations_view = v) },
            }
        }
        SettingRow { label: t("look.calendar"),
            Segmented {
                options: look_seg(CalendarView::ALL, CalendarView::id, CalendarView::label_key),
                selected: look.calendar_view.id().to_owned(),
                label: t("look.calendar"),
                on_select: move |id: String| if let Some(v) = CalendarView::from_id(&id) { with(&|l| l.calendar_view = v) },
            }
        }
        SettingRow { label: t("look.hero_autoplay"), hint: t("look.hero_autoplay.hint"),
            div { class: "tk-look__autoplay",
                if look.hero_autoplay {
                    Segmented {
                        options: HERO_STEPS.iter().map(|s| (s.to_string(), format!("{s} s"))).collect::<Vec<_>>(),
                        selected: nearest_step(look.hero_interval_secs).to_string(),
                        label: t("look.hero_interval"),
                        small: true,
                        on_select: move |id: String| if let Ok(v) = id.parse::<u8>() { with(&|l| l.hero_interval_secs = v) },
                    }
                }
                Switch {
                    checked: look.hero_autoplay,
                    label: t("look.hero_autoplay"),
                    on_toggle: move |v: bool| with(&|l| l.hero_autoplay = v),
                }
            }
        }
        SettingRow { label: t("look.greeting"),
            Switch {
                checked: look.show_greeting,
                label: t("look.greeting"),
                on_toggle: move |v: bool| with(&|l| l.show_greeting = v),
            }
        }
    }
}

/// Seconds offered for the hero autoplay (within `HERO_INTERVAL_MIN..=HERO_INTERVAL_MAX`).
const HERO_STEPS: [u8; 5] = [6, 9, 12, 15, 20];

/// The offered step closest to a stored value (a hand-edited file may hold any value in range).
fn nearest_step(v: u8) -> u8 {
    let v = v.clamp(HERO_INTERVAL_MIN, HERO_INTERVAL_MAX);
    HERO_STEPS.iter().copied().min_by_key(|s| s.abs_diff(v)).unwrap_or(9)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hero_steps_snap_to_the_nearest() {
        assert_eq!(nearest_step(9), 9);
        assert_eq!(nearest_step(10), 9);
        assert_eq!(nearest_step(14), 15);
        assert_eq!(nearest_step(200), 20);
        assert!(HERO_STEPS.iter().all(|s| (HERO_INTERVAL_MIN..=HERO_INTERVAL_MAX).contains(s)));
    }
}
