//! Torrent client > Library: saved torrents: series, episode, files, size, state, date; select / delete / open folder (`torrent_library`). Owner: wave 3 D (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentLibraryPage() -> Element {
    let section = TorrentSection::Library;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
