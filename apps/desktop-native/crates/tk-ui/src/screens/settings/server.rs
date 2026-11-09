//! Settings > Server & network (wave B, worker **k**; checklist `server.settings-ui`, `server.proxy`,
//! `server.flaresolverr`, `sources.embed-sniffer.timeout-setting`; classic `HomeServerSettings.tsx`,
//! `ProxySettingsPanel.tsx`, `FlareSolverrSettings.tsx`): start with the app, port, name, pairing required, media proxy,
//! timeouts / concurrency, FlareSolverr URL + "Test" (`flaresolverr_test`, `data-flaresolverr`), the embed sniffer
//! switch and timeout 5..=60 s (`data-sniffer`, `data-sniffer-timeout`), and "Open the server page" ->
//! `Route::ServerPage`. `SettingsAction::SetServer`. Contract placeholder.

use dioxus::prelude::*;

use super::SettingRow;
use crate::i18n::t;
use crate::model::SettingsAction;

#[component]
pub fn ServerSection(commit_saved: Callback<SettingsAction>) -> Element {
    let _ = commit_saved;
    rsx! {
        p { class: "tk-panel__hint", {t("settings.server.hint")} }
        SettingRow { label: t("settings.server.placeholder"), hint: t("common.placeholder") }
    }
}
