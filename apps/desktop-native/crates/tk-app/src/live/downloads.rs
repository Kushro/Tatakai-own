//! Download engine, Downloads page, local library and auto-download (wave B, worker **h**;
//! docs/redesign/16-parity-wave-b-contract.md section 4; checklist `downloads.*`). Contract stubs: h replaces every
//! body. Engine code lives in `tk_core::download` (HLS playlist walk with `m3u8-rs`, segment concat, MP4 `Range`
//! resume, queue with `Settings::downloads.concurrent`), rows in `LocalStore` (`tk-core/src/store/downloads.rs`:
//! `downloads`, `local_library`, `auto_download_runs`), files under `Settings::downloads.dir` (default
//! `<user Downloads>/Tatakai/Episodes`; `AppPaths::default_download_dir`). Manga / novel chapter rows
//! (`DownloadKindVm::MangaChapter` / `NovelChapter`) are enqueued by l / m through `manga_download_chapters` /
//! `novel_download_chapters` and executed here (pages / text are fetched through the extension host).

use std::sync::Arc;

use tk_core::LocalStore;
use tk_ui::backend::{
    AutoDownloadRunVm, DownloadRequestVm, DownloadStorageVm, DownloadVm, LibraryScanVm, LocalItemVm, PlaybackVm, UiResult,
};
use tk_ui::model::{Settings, UiError};

use crate::live::LiveBackend;
use crate::paths::AppPaths;

/// The engine: queue, workers, local-library index and the auto-download timer. Constructed once by
/// `LiveBackend::new`; h fills it (handles, `tokio::sync::Mutex` state, `notify` watcher of the library dirs).
pub struct Downloads {
    #[allow(dead_code)]
    paths: AppPaths,
    #[allow(dead_code)]
    store: Arc<LocalStore>,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl Downloads {
    pub fn new(paths: AppPaths, store: Arc<LocalStore>, client: reqwest::Client) -> Self {
        Self { paths, store, client }
    }
}

/// Start-up: resume `Queued` / `Downloading` rows from their last complete segment (`downloads.resume`), verify
/// completed rows (`downloads.integrity-check`) and arm the auto-download timer (`downloads.auto.schedule`). No-op
/// until h lands.
pub async fn on_start(_b: &LiveBackend) {}

/// `Settings::downloads` changed (folder, limits, auto-download, library dirs). No-op until h lands.
pub async fn on_settings(_b: &LiveBackend, _settings: &Settings) {}

/// Called by `LiveBackend::playback` first: a completed download or local-library file of `episode` plays offline
/// (`downloads.local-library.play-offline`). `Ok(None)` = nothing on disk (the normal online path follows). Until h
/// lands this is the only non-`NotWired` body here: it must never block playback.
pub async fn local_playback(_b: &LiveBackend, _media_id: &str, _episode: u32) -> UiResult<Option<PlaybackVm>> {
    Ok(None)
}

pub async fn downloads(_b: &LiveBackend) -> UiResult<Vec<DownloadVm>> {
    Err(UiError::not_wired("downloads"))
}
pub async fn enqueue(_b: &LiveBackend, _media_id: &str, _episode: u32, _req: &DownloadRequestVm) -> UiResult<i64> {
    Err(UiError::not_wired("download_enqueue"))
}
pub async fn enqueue_many(_b: &LiveBackend, _media_id: &str, _episodes: &[u32], _req: &DownloadRequestVm) -> UiResult<Vec<i64>> {
    Err(UiError::not_wired("download_enqueue_many"))
}
pub async fn pause(_b: &LiveBackend, _id: i64) -> UiResult<()> {
    Err(UiError::not_wired("download_pause"))
}
pub async fn resume(_b: &LiveBackend, _id: i64) -> UiResult<()> {
    Err(UiError::not_wired("download_resume"))
}
pub async fn cancel(_b: &LiveBackend, _id: i64, _delete_file: bool) -> UiResult<()> {
    Err(UiError::not_wired("download_cancel"))
}
pub async fn remove(_b: &LiveBackend, _id: i64, _delete_file: bool) -> UiResult<()> {
    Err(UiError::not_wired("download_remove"))
}
pub async fn clear_history(_b: &LiveBackend) -> UiResult<u32> {
    Err(UiError::not_wired("download_clear_history"))
}
pub async fn open_folder(_b: &LiveBackend, _id: i64) -> UiResult<()> {
    Err(UiError::not_wired("download_open_folder"))
}
pub async fn storage(_b: &LiveBackend) -> UiResult<DownloadStorageVm> {
    Err(UiError::not_wired("download_storage"))
}
pub async fn local_library(_b: &LiveBackend) -> UiResult<Vec<LocalItemVm>> {
    Err(UiError::not_wired("local_library"))
}
pub async fn library_scan(_b: &LiveBackend, _dir: Option<&str>) -> UiResult<LibraryScanVm> {
    Err(UiError::not_wired("library_scan"))
}
pub async fn library_import(_b: &LiveBackend, _paths: &[String]) -> UiResult<LibraryScanVm> {
    Err(UiError::not_wired("library_import"))
}
pub async fn local_match(_b: &LiveBackend, _item_id: i64, _media_id: &str, _episode: u32) -> UiResult<()> {
    Err(UiError::not_wired("local_match"))
}
pub async fn local_remove(_b: &LiveBackend, _item_id: i64, _delete_file: bool) -> UiResult<()> {
    Err(UiError::not_wired("local_remove"))
}
pub async fn auto_check_now(_b: &LiveBackend) -> UiResult<AutoDownloadRunVm> {
    Err(UiError::not_wired("auto_download_check_now"))
}
pub async fn auto_last_run(_b: &LiveBackend) -> UiResult<Option<AutoDownloadRunVm>> {
    Err(UiError::not_wired("auto_download_last_run"))
}
pub async fn verify(_b: &LiveBackend, _id: i64) -> UiResult<DownloadVm> {
    Err(UiError::not_wired("download_verify"))
}
