//! Settings > Data & storage: data directory, telemetry (none), clear cache, open folder, export history (wave A: `Backend::storage_usage`, `clear_cache`, `open_data_folder`, `export_history`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **f**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::{SettingRow, with_data_dir};
use crate::app::Services;
use crate::components::{Button, ButtonSize, ButtonVariant, Pill, Tone};
use crate::i18n::t;

#[component]
pub fn DataSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let data_hint = with_data_dir(&backend.data_dir_display(), "settings.data.hint_at", "settings.data.hint");
    rsx! {
        p { class: "tk-panel__hint", "{data_hint}" }
        SettingRow { label: t("settings.data.telemetry"), hint: t("settings.data.telemetry_hint"),
            Pill { text: t("settings.data.telemetry_none"), tone: Tone::Ok }
        }
        // No backend calls exist for these yet (docs/redesign/08): shown disabled, not faked.
        div { class: "tk-panel__row",
            Button {
                label: t("settings.data.clear_cache"),
                icon: "trash".to_string(),
                variant: ButtonVariant::Outlined,
                size: ButtonSize::Sm,
                disabled: true,
            }
            Button {
                label: t("settings.data.open_folder"),
                icon: "folder".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                disabled: true,
            }
            Button {
                label: t("settings.data.export"),
                icon: "download".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                disabled: true,
            }
        }
    }
}
