//! Torrent client settings persistence and the background-download / after-watch policy (docs/redesign/10
//! section 4.3). Owner: wave 3 D.
//!
//! `<data>/torrent-settings.json` holds `TorrentSettingsVm` (serde, `#[serde(default)]`, `sanitized()` on load and
//! save, atomic write like `settings_store`). Contract stubs: `load` returns the defaults, the rest is NotWired.

use tk_ui::backend::{TorrentSettingsVm, UiResult};
use tk_ui::model::UiError;

use super::LiveBackend;
use crate::paths::AppPaths;

/// Saved settings (defaults when missing or unreadable), sanitized, with `default_download_dir` filled. Also read by
/// `torrent::TorrentService` (A) to start the engine.
pub fn load(paths: &AppPaths) -> TorrentSettingsVm {
    TorrentSettingsVm {
        default_download_dir: paths.default_torrent_download_dir().display().to_string(),
        ..TorrentSettingsVm::default()
    }
}

/// Persist `settings.clone().sanitized()`.
pub fn save(paths: &AppPaths, settings: &TorrentSettingsVm) -> UiResult<()> {
    let _ = (paths, settings);
    Err(UiError::not_wired("save_torrent_settings"))
}

/// `Backend::torrent_settings`.
pub async fn settings(b: &LiveBackend) -> UiResult<TorrentSettingsVm> {
    Ok(load(&b.paths()))
}

/// `Backend::save_torrent_settings`: `save` then `b.torrent.apply_settings`.
pub async fn save_settings(b: &LiveBackend, settings: &TorrentSettingsVm) -> UiResult<()> {
    let _ = (b, settings);
    Err(UiError::not_wired("save_torrent_settings"))
}

/// `Backend::background_downloads_ack`: persist `background_ack = true`.
pub async fn background_ack(b: &LiveBackend) -> UiResult<()> {
    let _ = b;
    Err(UiError::not_wired("background_downloads_ack"))
}

/// `Backend::background_downloads_enabled`.
pub async fn background_enabled(b: &LiveBackend) -> UiResult<bool> {
    let s = load(&b.paths());
    Ok(s.background_ack && s.background_downloads)
}

/// `Backend::torrent_add_download`: refuse unless `background_enabled`; `torrent_play::resolve_source` (B) ->
/// `engine.add(.., AddOptions { persist: true, only_files: the episode file when `episode` is `Some`, .. })`.
pub async fn add_download(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>) -> UiResult<String> {
    let _ = (b, source, id, episode);
    Err(UiError::not_wired("torrent_add_download"))
}

/// `Backend::torrent_playback_ended`: torrents not kept (`persist == false`, i.e. after_watch Delete when it was
/// added) are removed with their files unless a background download of the same torrent is running; kept ones
/// pause when background downloads are off (so nothing uploads while nothing plays) and keep seeding otherwise.
pub async fn playback_ended(b: &LiveBackend, torrent_id: &str, completed: bool) -> UiResult<()> {
    let _ = (b, torrent_id, completed);
    Err(UiError::not_wired("torrent_playback_ended"))
}
