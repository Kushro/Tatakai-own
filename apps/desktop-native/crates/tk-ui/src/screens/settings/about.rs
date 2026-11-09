//! Settings > About: version, Link version, license; wave A wires Check for updates, Licenses (sheet with `THIRD_PARTY_NOTICES.md`) and Open logs (`Backend::check_updates`, `licenses_text`, `open_logs`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **f**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use crate::components::{Button, ButtonSize, ButtonVariant, use_snackbar};
use crate::i18n::t;

#[component]
pub fn AboutSection() -> Element {
    let snack = use_snackbar();
    let version = format!("{} · tk-app (Dioxus 0.7)", env!("CARGO_PKG_VERSION"));
    rsx! {
        div { class: "tk-about",
            p { {t("settings.about.body")} }
            dl {
                dt { "Tatakai Next" }
                dd { "{version}" }
                dt { "Tatakai-Link" }
                dd { "v1 · openapi.v1.json" }
                dt { {t("settings.about.license")} }
                dd { "AGPL-3.0-or-later" }
            }
        }
        div { class: "tk-panel__row",
            Button {
                label: t("settings.about.check_updates"),
                icon: "update".to_string(),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                disabled: true,
            }
            Button {
                label: t("settings.about.licenses"),
                icon: "external".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                onclick: move |_| snack.show("THIRD_PARTY_NOTICES.md"),
            }
            Button {
                label: t("settings.about.logs"),
                icon: "terminal".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                disabled: true,
            }
        }
    }
}
