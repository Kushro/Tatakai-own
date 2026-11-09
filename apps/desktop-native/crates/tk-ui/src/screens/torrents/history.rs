//! Torrents > History (wave A, worker **i**; plan 14 section 0.1 #11, checklist `torrent.session-history`): every
//! torrent the engine ran, newest first (`Backend::torrent_session_history`), with series / episode, size moved
//! (downloaded / uploaded), started / ended, completed; "Open in client" for rows still in the engine, "Add again"
//! for closed ones (`torrent_add_manual` with the stored info-hash), and "Clear history" (`torrent_clear_session_history`,
//! `ConfirmDialog`). Route `/torrents/history`. Rows expose `data-session-id` and `data-session-open="true|false"`.
//!
//! Contract placeholder (docs/redesign/15-parity-wave-a-contract.md section 4): the owner replaces this file.
//! Every state needs loading (`LoadingIndicator` / skeleton), empty (`EmptyState`) and error (`ErrorState`).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn TorrentHistoryPage() -> Element {
    rsx! {
        div { class: "tk-page",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("torrent.section.history")} }
            }
            EmptyState { icon: "history".to_string(), title: t("torrent.section.history"), body: t("common.placeholder") }
        }
    }
}
