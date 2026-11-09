//! Torrents > Debrid (wave A, worker **i**): Real-Debrid, TorBox, AllDebrid and Premiumize API keys with validation and account status (`Backend::debrid_accounts`, `set_debrid_key`); once connected, torrent sources show the cached pill and play through the service. Route `/torrents/debrid`.
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn TorrentDebridPage() -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("torrent.section.debrid")} }
            }
            EmptyState { icon: "info".to_string(), title: t("torrent.section.debrid"), body: t("common.placeholder") }
        }
    }
}
