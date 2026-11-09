//! Native extension server page `/server` (wave B, worker **k**; checklist `server.native-mode`, `server.settings-ui`,
//! `server.pairing`, `server.devices`, `server.viewer-password`, `server.logs`, `server.flaresolverr`, `server.proxy`;
//! classic `settings/HomeServerSettings.tsx`, `FlareSolverrSettings.tsx`, `ProxySettingsPanel.tsx`): status card
//! (`Backend::server_status`, start / stop, `data-server-running`, `data-server-port`), pairing (QR + code + deep link,
//! `server_open_pairing`), devices (`server_devices`, revoke), viewer password (`server_set_viewer_password`), logs
//! (`server_logs`, `data-server-log-cursor`), proxy / cache (`server_proxy_status`, `server_clear_cache`), FlareSolverr
//! test (`flaresolverr_test`, `data-flaresolverr="ok|error"`). Linked from the Link page and Settings > Server. Contract
//! placeholder: k replaces the body.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn ServerPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "server.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("server.title")} }
            }
            EmptyState { icon: "desktop".to_string(), title: t("server.title"), body: t("common.placeholder") }
        }
    }
}
