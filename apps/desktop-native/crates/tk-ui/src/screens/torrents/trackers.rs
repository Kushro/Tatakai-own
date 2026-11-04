//! Torrent client > Trackers: table url / status / downloaded / seeders / leechers (`torrent_trackers`). Owner: wave 3 C (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentTrackersPage() -> Element {
    let section = TorrentSection::Trackers;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
