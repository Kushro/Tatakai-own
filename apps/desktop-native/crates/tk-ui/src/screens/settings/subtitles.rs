//! Settings > Subtitles: style (size, font, colour, background, outline, position), secondary subtitle language, translation provider + key + target language + test, live / pre-translation toggles (`SettingsAction::SetSubs`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **d**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use crate::i18n::t;

/// Contract placeholder: the owner replaces the body. Keys for this section go to the `subs` namespace.
#[component]
pub fn SubtitlesSection() -> Element {
    rsx! {
        p { class: "tk-panel__hint", {t("common.placeholder")} }
    }
}
