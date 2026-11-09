//! `Backend` system calls (wave A, worker **f**; docs/redesign/15 section 4): data & storage, about, OS
//! integration, first-run wizard support, "reset app" and the start-up jobs. Bodies: `storage_usage` / `clear_cache`
//! walk `b.paths()` (`cache` rows of the store + `webview/Cache` + `<data>/images` + `translation_cache`); `open_*` =
//! `open::that_detached`; `export_history` writes JSON / CSV from `b.store.history(10_000)` into
//! `directories::UserDirs::download_dir`; `licenses_text` = `include_str!("../../../../THIRD_PARTY_NOTICES.md")`;
//! `check_updates` = `crate::update_check::latest` (GitHub `releases/latest` of `Kushro/Tatakai-own`) + `semver`;
//! autostart = `crate::autostart`; `reset_app` stops Link (`b.link`) and the torrent engine (`b.torrent`), deletes the
//! keyring entries (`b.secrets.delete` for every `tk_core::secrets::keys`), then removes the contents of
//! `b.paths().root` except the download folder when `keep_downloads` (never anything outside those two places) and
//! finally asks the UI to restart (the window reloads with defaults: `Settings::system.setup_done` is false, so the
//! wizard shows). `on_start` runs once per launch from `Services::on_start`: the update check when
//! `Settings::system.check_updates`; the result is kept in a private `OnceLock<UpdateCheckVm>` of this module that
//! `check_updates` answers first (no second request), and the shell's start-up snackbar (`update.*` keys, "Open" /
//! "Skip this version" -> `skipped_version`) reads it through `Backend::check_updates`. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use std::path::PathBuf;

use tk_ui::backend::{ExportFormatVm, StorageUsageVm, SystemCapsVm, UpdateCheckVm};
use tk_ui::model::PlatformVm;
use crate::live::LiveBackend;

/// `Backend::reset_app`.
pub async fn reset_app(_b: &LiveBackend, _keep_downloads: bool) -> UiResult<()> {
    Err(UiError::not_wired("reset_app"))
}

/// `Backend::storage_usage`.
pub async fn storage_usage(_b: &LiveBackend) -> UiResult<StorageUsageVm> {
    Err(UiError::not_wired("storage_usage"))
}

/// `Backend::clear_cache` (bytes freed).
pub async fn clear_cache(_b: &LiveBackend) -> UiResult<u64> {
    Err(UiError::not_wired("clear_cache"))
}

/// `Backend::open_data_folder`.
pub async fn open_data_folder(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("open_data_folder"))
}

/// `Backend::open_logs`.
pub async fn open_logs(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("open_logs"))
}

/// `Backend::export_history`.
pub async fn export_history(_b: &LiveBackend, _format: ExportFormatVm) -> UiResult<PathBuf> {
    Err(UiError::not_wired("export_history"))
}

/// `Backend::licenses_text`.
pub async fn licenses_text(_b: &LiveBackend) -> UiResult<String> {
    Err(UiError::not_wired("licenses_text"))
}

/// `Backend::check_updates`.
pub async fn check_updates(_b: &LiveBackend) -> UiResult<UpdateCheckVm> {
    Err(UiError::not_wired("check_updates"))
}

/// `Backend::autostart_enabled` (`crate::autostart::is_enabled`).
pub async fn autostart_enabled(_b: &LiveBackend) -> UiResult<bool> {
    Err(UiError::not_wired("autostart_enabled"))
}

/// `Backend::set_autostart` (`crate::autostart::set_enabled`).
pub async fn set_autostart(_b: &LiveBackend, _enabled: bool) -> UiResult<()> {
    Err(UiError::not_wired("set_autostart"))
}

/// `Backend::open_url` (`http(s)` only; anything else is `ErrorKind::Other`).
pub async fn open_url(_b: &LiveBackend, _url: &str) -> UiResult<()> {
    Err(UiError::not_wired("open_url"))
}

/// `Backend::system_caps` (sync). Contract stub: nothing offered; f returns `autostart: true` where
/// `crate::autostart` supports the platform, `tray: true` where `tk_ui::tray` can show an icon and
/// `magnet_handler: crate::magnet::supported()` (worker i's module; f only reads it).
pub fn system_caps(_b: &LiveBackend) -> SystemCapsVm {
    SystemCapsVm { platform: PlatformVm::current(), autostart: false, tray: false, magnet_handler: false }
}

/// Start-up job (called once by `LiveBackend::on_start`, spawned by tk-app's `main.rs` through `Services::on_start`):
/// the new-version notice (`crate::update_check`, respecting `Settings::system.check_updates` / `skipped_version`).
/// Worker i's `magnet::from_args` is consumed by `torrent_manual::take_pending_magnet`, not here. Contract stub.
pub async fn on_start(_b: &LiveBackend) {}
