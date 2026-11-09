//! `Backend` manual torrents + preload (wave A, worker **i**; docs/redesign/15 section 4). Bodies: `torrent_preview` adds the torrent paused through `b.torrent.engine()` (`AddOptions { paused: true, persist: false, .. }`), reads `files`, runs `tk_core::release::parse_release` per video file for the suggested alignment, and removes it again unless `torrent_add_manual` follows within 5 min (keep the id in a `Mutex` here); `torrent_add_manual` sets persist + label + `b.store.set_torrent_episode_map`; `torrent_play` then consults the map before `pick_episode_file` (i edits `live/torrent_play.rs`, which it owns in wave A). `torrent_preload_next` reuses `torrent_settings::add_download` for the next episode when `TorrentSettingsVm::preload_next`. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::TorrentPreviewVm;
use crate::live::LiveBackend;

/// `Backend::torrent_preview`.
pub async fn torrent_preview(_b: &LiveBackend, _source: &str) -> UiResult<TorrentPreviewVm> {
    Err(UiError::not_wired("torrent_preview"))
}

/// `Backend::torrent_add_manual` (returns the torrent id).
pub async fn torrent_add_manual(_b: &LiveBackend, _source: &str, _media_id: Option<&str>, _alignment: &[(usize, u32)]) -> UiResult<String> {
    Err(UiError::not_wired("torrent_add_manual"))
}

/// `Backend::torrent_preload_next`.
pub async fn torrent_preload_next(_b: &LiveBackend, _id: &str, _episode: u32) -> UiResult<()> {
    Err(UiError::not_wired("torrent_preload_next"))
}
