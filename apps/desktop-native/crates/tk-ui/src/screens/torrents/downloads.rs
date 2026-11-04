//! Torrent client > Downloads: background downloads behind the I-understand warning (`background_downloads_ack`). Owner: wave 3 D (contract placeholder).

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;
use crate::model::TorrentSection;

#[component]
pub fn TorrentDownloadsPage() -> Element {
    let section = TorrentSection::Downloads;
    rsx! {
        EmptyState { icon: section.icon().to_string(), title: t(section.label_key()), body: t("torrent.placeholder") }
    }
}
