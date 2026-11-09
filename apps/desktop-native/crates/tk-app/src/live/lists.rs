//! `Backend` list calls (wave A, worker **g**; docs/redesign/15 section 4). Bodies over `tk_core::lists` (OAuth, tokens in `b.secrets`), `tk_core::metadata::anilist::lists` / `tk_core::lists::mal`, and `b.store` (`store::lists`): local mirror first, offline queue, push when online, conflicts when both sides changed since the last sync. `on_progress` is the hook `LiveBackend::record_progress` already calls: when `Settings::lists.auto_progress` and the episode passed `progress_at`, bump the entry's progress (and status to `watching` / `completed`) through `set_list_entry`. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use std::path::PathBuf;

use tk_ui::backend::{ConflictSideVm, ExportFormatVm, ListAccountVm, ListConflictVm, ListEntryUpdateVm, ListEntryVm, ListImportVm, ListStatusVm, ListSyncVm, MediaVm};
use tk_ui::model::ListServiceVm;
use crate::live::LiveBackend;

/// `Backend::list_accounts`.
pub async fn list_accounts(_b: &LiveBackend) -> UiResult<Vec<ListAccountVm>> {
    Err(UiError::not_wired("list_accounts"))
}

/// `Backend::list_connect` (opens the browser with `open::that_detached`, waits on `tk_core::lists::oauth::wait_for_callback`).
pub async fn list_connect(_b: &LiveBackend, _service: ListServiceVm) -> UiResult<ListAccountVm> {
    Err(UiError::not_wired("list_connect"))
}

/// `Backend::list_disconnect`.
pub async fn list_disconnect(_b: &LiveBackend, _service: ListServiceVm) -> UiResult<()> {
    Err(UiError::not_wired("list_disconnect"))
}

/// `Backend::list_entry`.
pub async fn list_entry(_b: &LiveBackend, _media_id: &str) -> UiResult<Option<ListEntryVm>> {
    Err(UiError::not_wired("list_entry"))
}

/// `Backend::set_list_entry`.
pub async fn set_list_entry(_b: &LiveBackend, _media_id: &str, _update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
    Err(UiError::not_wired("set_list_entry"))
}

/// `Backend::remove_list_entry`.
pub async fn remove_list_entry(_b: &LiveBackend, _media_id: &str) -> UiResult<()> {
    Err(UiError::not_wired("remove_list_entry"))
}

/// `Backend::list_entries`.
pub async fn list_entries(_b: &LiveBackend, _status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
    Err(UiError::not_wired("list_entries"))
}

/// `Backend::favorites`.
pub async fn favorites(_b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Err(UiError::not_wired("favorites"))
}

/// `Backend::is_favorite`.
pub async fn is_favorite(_b: &LiveBackend, _media_id: &str) -> UiResult<bool> {
    Err(UiError::not_wired("is_favorite"))
}

/// `Backend::set_favorite`.
pub async fn set_favorite(_b: &LiveBackend, _media_id: &str, _favorite: bool) -> UiResult<()> {
    Err(UiError::not_wired("set_favorite"))
}

/// `Backend::list_sync_now`.
pub async fn list_sync_now(_b: &LiveBackend) -> UiResult<ListSyncVm> {
    Err(UiError::not_wired("list_sync_now"))
}

/// `Backend::list_conflicts`.
pub async fn list_conflicts(_b: &LiveBackend) -> UiResult<Vec<ListConflictVm>> {
    Err(UiError::not_wired("list_conflicts"))
}

/// `Backend::resolve_list_conflict`.
pub async fn resolve_list_conflict(_b: &LiveBackend, _conflict_id: i64, _keep: ConflictSideVm) -> UiResult<()> {
    Err(UiError::not_wired("resolve_list_conflict"))
}

/// `Backend::import_list`.
pub async fn import_list(_b: &LiveBackend, _path: &str) -> UiResult<ListImportVm> {
    Err(UiError::not_wired("import_list"))
}

/// `Backend::export_list`.
pub async fn export_list(_b: &LiveBackend, _format: ExportFormatVm) -> UiResult<PathBuf> {
    Err(UiError::not_wired("export_list"))
}

/// Hook called by `LiveBackend::record_progress` after the local history write (never fails the write). Contract
/// stub: no-op. Worker g implements the auto-progress rule here.
pub async fn on_progress(_b: &LiveBackend, _id: &str, _episode: u32, _progress_secs: u32, _duration_secs: Option<u32>, _completed: bool) {}
