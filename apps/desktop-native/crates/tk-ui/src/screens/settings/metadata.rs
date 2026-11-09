//! Settings > Metadata: provider priority and cache.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: nobody in wave A (integrator). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::{SettingRow, provider_mono};
use crate::app::Ui;
use crate::components::{IconButton, IconButtonSize};
use crate::i18n::{t, tf};
use crate::model::SettingsAction;

#[component]
pub fn MetadataSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let s = (ui.settings)();
    rsx! {
        p { class: "tk-panel__hint", {t("settings.metadata.hint")} }
        ol { class: "tk-priority",
            for (i , provider) in s.provider_priority.iter().enumerate() {
                {
                    let name = t(&format!("provider.{provider}"));
                    let sub = t(&format!("provider.{provider}.sub"));
                    let mono = provider_mono(provider);
                    let last = i + 1 == s.provider_priority.len();
                    rsx! {
                        li { key: "{provider}", class: "tk-priority__row",
                            span { class: "tk-priority__rank", "{i + 1}" }
                            span { class: "tk-priority__ico", "aria-hidden": "true", "{mono}" }
                            span { class: "tk-priority__name",
                                "{name}"
                                small { "{sub}" }
                            }
                            IconButton {
                                icon: "chevron-up".to_string(),
                                label: tf("settings.metadata.up", &[("name", &name)]),
                                size: IconButtonSize::Sm,
                                disabled: i == 0,
                                onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: -1 }),
                            }
                            IconButton {
                                icon: "chevron-down".to_string(),
                                label: tf("settings.metadata.down", &[("name", &name)]),
                                size: IconButtonSize::Sm,
                                disabled: last,
                                onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: 1 }),
                            }
                        }
                    }
                }
            }
        }
        SettingRow { label: t("settings.metadata.cache"), hint: t("settings.metadata.cache_hint") }
    }
}
