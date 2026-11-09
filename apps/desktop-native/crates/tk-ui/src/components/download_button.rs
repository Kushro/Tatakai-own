//! Player "Download" button (wave B, worker **h**; checklist `downloads.player-button`; classic
//! `video/controls/DownloadButton.tsx`). Mounted by `screens/player/mod.rs` next to `PlayerControls` (sibling, class
//! `tk-player__download`, hidden with the controls): one click calls `Backend::download_enqueue(media_id, episode,
//! &DownloadRequestVm { source_id, .. })` and confirms with a snackbar; torrent sources go through
//! `torrent_add_download` instead. `data-download-state="idle|queued|done"`. Props are final; renders nothing until h.

use dioxus::prelude::*;

#[component]
pub fn PlayerDownloadButton(
    media_id: String,
    episode: u32,
    /// `SourceVm::id` of the playing source; `None` while resolving (the button is disabled).
    #[props(default)] source_id: Option<String>,
    /// Follows the controls' visibility.
    #[props(default = true)] visible: bool,
) -> Element {
    let _ = (media_id, episode, source_id, visible);
    rsx! {}
}
