//! Torrent engine wiring (docs/redesign/10 section 4.1). Owner: wave 3 A.
//!
//! `TorrentService` owns the `tk_torrent::TorrentEngine` of the app: started on demand (first `engine()` call: playing
//! a torrent, opening the torrent client, or at start-up when background downloads are enabled), configured from
//! `TorrentSettingsVm` (`torrent_settings::load`) and shut down cleanly (Drop / app exit). It also maps the engine
//! types to the UI view models. Contract stubs: every call fails with `ErrorKind::NotWired`.

use std::sync::Arc;

use tk_torrent::{EngineConfig, TorrentEngine, TorrentError};
use tk_ui::backend::{
    TorrentFileVm, TorrentLibraryItemVm, TorrentPeerVm, TorrentSettingsVm, TorrentStatsVm, TorrentTrackerVm, UiResult,
};
use tk_ui::model::{ErrorKind, UiError};

use crate::paths::AppPaths;

pub struct TorrentService {
    paths: AppPaths,
}

impl TorrentService {
    /// Cheap: nothing starts until `engine()`.
    pub fn new(paths: AppPaths) -> Self {
        Self { paths }
    }

    /// The running engine, starting it on first use with `engine_config(&torrent_settings::load(..), ..)`.
    pub async fn engine(&self) -> UiResult<Arc<TorrentEngine>> {
        let _ = &self.paths;
        Err(UiError::not_wired("torrent engine"))
    }

    /// The engine if it is already running (never starts it): stats polls of a closed client cost nothing.
    pub fn running(&self) -> Option<Arc<TorrentEngine>> {
        None
    }

    /// Apply saved settings: limits live (`set_limits`); port / DHT / PEX / LSD / UPnP / folder changes restart the
    /// engine when no stream is open, else on the next start.
    pub async fn apply_settings(&self, _settings: &TorrentSettingsVm) -> UiResult<()> {
        Ok(())
    }

    pub async fn stats(&self, _id: &str) -> UiResult<TorrentStatsVm> {
        Err(UiError::not_wired("torrent_stats"))
    }
    pub async fn files(&self, _id: &str) -> UiResult<Vec<TorrentFileVm>> {
        Err(UiError::not_wired("torrent_files"))
    }
    pub async fn peers(&self, _id: &str) -> UiResult<Vec<TorrentPeerVm>> {
        Err(UiError::not_wired("torrent_peers"))
    }
    pub async fn trackers(&self, _id: &str) -> UiResult<Vec<TorrentTrackerVm>> {
        Err(UiError::not_wired("torrent_trackers"))
    }
    pub async fn library(&self) -> UiResult<Vec<TorrentLibraryItemVm>> {
        Err(UiError::not_wired("torrent_library"))
    }
    pub async fn remove(&self, _id: &str, _delete_files: bool) -> UiResult<()> {
        Err(UiError::not_wired("torrent_remove"))
    }
    pub async fn pause(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_pause"))
    }
    pub async fn resume(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_resume"))
    }
    pub async fn set_persist(&self, _id: &str, _persist: bool) -> UiResult<()> {
        Err(UiError::not_wired("torrent_set_persist"))
    }
    /// `xdg-open <save_path>` on Linux, `explorer <save_path>` on Windows (no shell, no console window).
    pub async fn open_folder(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_open_folder"))
    }
    pub async fn seek_marks(&self, _id: &str, _file_index: usize) -> UiResult<Vec<(f64, f64)>> {
        Err(UiError::not_wired("seek_marks"))
    }
}

/// `TorrentSettingsVm` -> engine config (empty `download_dir` = `AppPaths::default_torrent_download_dir`, KiB/s ->
/// B/s, 0 = unlimited, `persistence_dir` = `AppPaths::torrent_dir`).
pub fn engine_config(settings: &TorrentSettingsVm, paths: &AppPaths) -> EngineConfig {
    let _ = settings;
    EngineConfig::new(paths.default_torrent_download_dir(), paths.torrent_dir())
}

/// Engine errors -> UI errors (`NotFound` / `NoSuchFile` -> `NoSource`, `Timeout` -> `Offline` (no peers), the rest
/// `Other`), keeping the message as detail.
pub fn engine_err(e: TorrentError) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}
