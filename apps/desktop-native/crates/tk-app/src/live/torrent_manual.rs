//! `Backend` manual torrents, preload, OS magnet handler, session history and reannounce (wave A, worker **i**;
//! docs/redesign/15 section 4; plan 14 section 0.1 #11). Bodies: `torrent_preview` adds the torrent paused through
//! `b.torrent.engine()` (`AddOptions { paused: true, persist: false, .. }`), reads `files`, runs
//! `tk_core::release::parse_release` per video file for the suggested alignment, and removes it again unless
//! `torrent_add_manual` follows within 5 min (keep the id in a `Mutex` here); `torrent_add_manual` sets persist +
//! label + `b.store.set_torrent_episode_map`; `torrent_play` then consults the map before `pick_episode_file` (i edits
//! `live/torrent_play.rs`, which it owns in wave A). `torrent_preload_next` reuses `torrent_settings::add_download`
//! for the next episode when `TorrentSettingsVm::preload_next`. Magnet handler = `crate::magnet` (`take`,
//! `is_registered`, `set_registered`). Session history = `b.store.{open,close}_torrent_session` called from
//! `live/torrent.rs` when a torrent enters / leaves the engine (+ closing stale open rows at engine start) and
//! `torrent_sessions` / `clear_torrent_sessions` here. `torrent_reannounce` = librqbit's per-torrent tracker
//! re-announce (i checks the exact API in the vendored 9.1.0 source; if none exists, it re-adds the trackers, which
//! triggers an announce) and `live/torrent.rs::trackers` maps the real announce outcome to `TrackerStatusVm`
//! instead of `Unknown` (checklist `torrent.trackers.status`). Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::{TorrentPreviewVm, TorrentSessionVm};
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

/// `Backend::take_pending_magnet` (`crate::magnet::take`; already real, nothing to wire).
pub async fn take_pending_magnet(_b: &LiveBackend) -> UiResult<Option<String>> {
    Ok(crate::magnet::take())
}

/// `Backend::magnet_handler_registered` (`crate::magnet::is_registered`).
pub async fn magnet_handler_registered(_b: &LiveBackend) -> UiResult<bool> {
    Err(UiError::not_wired("magnet_handler_registered"))
}

/// `Backend::set_magnet_handler` (`crate::magnet::set_registered`; io errors become `ErrorKind::Other` with the message).
pub async fn set_magnet_handler(_b: &LiveBackend, _enabled: bool) -> UiResult<()> {
    Err(UiError::not_wired("set_magnet_handler"))
}

/// `Backend::torrent_session_history` (`b.store.torrent_sessions(limit)` mapped 1:1 to `TorrentSessionVm`).
pub async fn torrent_session_history(_b: &LiveBackend, _limit: u32) -> UiResult<Vec<TorrentSessionVm>> {
    Err(UiError::not_wired("torrent_session_history"))
}

/// `Backend::torrent_clear_session_history` (`b.store.clear_torrent_sessions()`).
pub async fn torrent_clear_session_history(_b: &LiveBackend) -> UiResult<u32> {
    Err(UiError::not_wired("torrent_clear_session_history"))
}

/// `Backend::torrent_reannounce`.
pub async fn torrent_reannounce(_b: &LiveBackend, _torrent_id: &str) -> UiResult<()> {
    Err(UiError::not_wired("torrent_reannounce"))
}
