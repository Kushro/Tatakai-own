//! Torrent client > Peers: table ip:port / client / progress / down / up / downloaded / uploaded / country / flags (`torrent_peers`). Owner: wave 3 C (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentPeersPage() -> Element {
    let section = TorrentSection::Peers;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
