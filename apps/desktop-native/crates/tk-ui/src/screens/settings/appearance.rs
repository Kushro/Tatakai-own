//! Settings > Appearance: palette, mode, contrast, language, title language (wave A, worker c), reduce motion.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: nobody in wave A (integrator). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::{SettingRow, seg};
use crate::app::Ui;
use crate::components::{Segmented, Switch};
use crate::i18n::{LangChoice, t};
use crate::model::{Contrast, PALETTES, SettingsAction, TITLE_LANGUAGES, ThemeMode};

#[component]
pub fn AppearanceSection(commit: Callback<SettingsAction>, commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let s = (ui.settings)();
    rsx! {
        SettingRow { label: t("settings.palette"), hint: t("settings.palette_hint"), stack: true,
            div { class: "tk-swatches", role: "radiogroup", "aria-label": t("settings.palette"),
                for p in PALETTES {
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
        SettingRow { label: t("settings.reduce_motion"), hint: t("settings.reduce_motion_hint"),
            Switch {
                checked: s.reduce_motion,
                label: t("settings.reduce_motion"),
                on_toggle: move |v: bool| commit_saved.call(SettingsAction::SetReduceMotion(v)),
            }
        }
    }
}
