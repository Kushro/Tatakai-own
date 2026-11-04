//! Torrent playback path (docs/redesign/10 section 4.2). Owner: wave 3 B.
//!
//! `play`: `resolve_source` -> `engine.add(source, AddOptions { persist: after_watch == Keep, label, .. })` (or the
//! torrent already in the session) -> `tk_core::release::pick_episode_file(files, episode, titles)` ->
//! `engine.select_file` -> `engine.stream_url` -> `TorrentPlaybackVm` (title / poster / start position like
//! `playback_source`). No file matched -> `ErrorKind::NoSource` with the torrent id as detail (the player then offers
//! the Files page of the client). Contract stubs: `ErrorKind::NotWired`.

use tk_torrent::{AddSource, TorrentLabel};
use tk_ui::backend::{SkipMarkVm, TorrentPlaybackVm, UiResult};
use tk_ui::model::UiError;

use super::LiveBackend;

/// `Backend::torrent_play`.
pub async fn play(b: &LiveBackend, source: &str, id: &str, episode: u32) -> UiResult<TorrentPlaybackVm> {
    let _ = (b, source, id, episode);
    Err(UiError::not_wired("torrent_play"))
}

/// What to add for `source` (a torrent `SourceVm::id` -> its candidate's magnet, else `magnet:?xt=urn:btih:<hash>`
/// with the candidate's trackers; a magnet; a 40-hex hash / torrent id) plus the label for the Library. Shared with
/// `torrent_settings::add_download` (D).
pub async fn resolve_source(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>) -> UiResult<(AddSource, TorrentLabel)> {
    let _ = (b, source, id, episode);
    Err(UiError::not_wired("torrent source resolution"))
}

/// `Backend::skip_marks`: AniSkip (`https://api.aniskip.com/v2/skip-times/<mal id>/<episode>?types=op&types=ed&
/// types=recap&episodeLength=0`, fixed host, short timeout, cached per episode); empty when the title has no MAL id
/// or AniSkip has no data.
pub async fn skip_marks(b: &LiveBackend, id: &str, episode: u32) -> UiResult<Vec<SkipMarkVm>> {
    let _ = (b, id, episode);
    Err(UiError::not_wired("skip_marks"))
}
