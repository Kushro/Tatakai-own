//! Torrent client > Overview: name, state pill, info-hash, progress, downloaded / uploaded / total / pieces, speeds, remaining or Streaming, seeders / leechers / wires, protocol pills (`torrent_stats`, polled every second). Owner: wave 3 C (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentOverviewPage() -> Element {
    let section = TorrentSection::Overview;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
