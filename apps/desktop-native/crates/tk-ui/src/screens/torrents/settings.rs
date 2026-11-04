//! Torrent client > Settings: speed limits, ports, download folder, after watching, max peers, DHT / PEX / LSD, background downloads (`torrent_settings`). Owner: wave 3 D (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentSettingsPage() -> Element {
    let section = TorrentSection::Settings;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
