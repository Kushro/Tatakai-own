//! `Backend` account + system calls (wave A, worker **f**; docs/redesign/15 section 4). Bodies: `b.supabase` + `tk_core::supabase::auth` for sign-up / verify / recovery (`session_store` save after sign-in, clear on sign-out: `LiveBackend::sign_in` / `sign_out` already call `after_sign_in` / `after_sign_out` below, and `LiveBackend::new` spawns `restore_session`); `storage_usage` / `clear_cache` walk `b.paths()` (`cache` rows of the store + `webview/Cache` + `<data>/images`); `open_*` = `open::that_detached`; `export_history` writes JSON / CSV from `b.store.history(10_000)` into `directories::UserDirs::download_dir`; `licenses_text` = `include_str!("../../../../THIRD_PARTY_NOTICES.md")`; `check_updates` = GitHub `releases/latest` of `Kushro/Tatakai-own` + `semver`; autostart = `crate::autostart`. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use std::path::PathBuf;

use tk_ui::backend::{AccountVm, ExportFormatVm, SignUpVm, StorageUsageVm, SystemCapsVm, UpdateCheckVm};
use tk_ui::model::PlatformVm;
use crate::live::LiveBackend;

/// `Backend::sign_up`.
pub async fn sign_up(_b: &LiveBackend, _email: &str, _password: &str) -> UiResult<SignUpVm> {
    Err(UiError::not_wired("sign_up"))
}

/// `Backend::verify_email`.
pub async fn verify_email(_b: &LiveBackend, _email: &str, _code: &str) -> UiResult<AccountVm> {
    Err(UiError::not_wired("verify_email"))
}

/// `Backend::resend_verification`.
pub async fn resend_verification(_b: &LiveBackend, _email: &str) -> UiResult<()> {
    Err(UiError::not_wired("resend_verification"))
}

/// `Backend::request_password_reset`.
pub async fn request_password_reset(_b: &LiveBackend, _email: &str) -> UiResult<()> {
    Err(UiError::not_wired("request_password_reset"))
}

/// `Backend::reset_password`.
pub async fn reset_password(_b: &LiveBackend, _email: &str, _code: &str, _new_password: &str) -> UiResult<()> {
    Err(UiError::not_wired("reset_password"))
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
/// `crate::autostart` supports the platform and `tray: true` where `tk_ui::tray` can show an icon.
pub fn system_caps(_b: &LiveBackend) -> SystemCapsVm {
    SystemCapsVm { platform: PlatformVm::current(), autostart: false, tray: false }
}

/// Called by `LiveBackend::sign_in` after a successful sign-in: persist the session through
/// `tk_core::supabase::session_store::save(&b.secrets, ..)`. Contract stub.
pub async fn after_sign_in(_b: &LiveBackend) {}

/// Counterpart of `after_sign_in` for `sign_out` (clears the persisted session). Contract stub.
pub async fn after_sign_out(_b: &LiveBackend) {}

/// Start-up: load the persisted session into `b.supabase` (`session_store::load` + `Supabase::set_session`, refresh
/// when near expiry). Called by `LiveBackend::restore_session` (spawned by tk-app's `main.rs`). Contract stub.
pub async fn restore_session(_b: &LiveBackend) {}
