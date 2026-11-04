//! Torrent client > Files: searchable table name / size / progress / streams (`torrent_files`). Owner: wave 3 C (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentFilesPage() -> Element {
    let section = TorrentSection::Files;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
