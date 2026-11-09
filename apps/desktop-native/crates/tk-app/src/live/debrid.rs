//! `Backend` debrid calls (wave A, worker **i**; docs/redesign/15 section 4). Bodies over `tk_core::debrid::DebridClient` with keys from `b.secrets` (`keys::debrid`), cache checks in `b.store` (`store::torrents::debrid_cached`, TTL 10 min). `mark_cached` is already called by `LiveBackend::sources` (`live/mod.rs`) on the ranked list: i only fills its body. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::{DebridAccountVm, DebridServiceVm, PlaybackVm, SourceVm};
use crate::live::LiveBackend;

/// `Backend::debrid_accounts`.
pub async fn debrid_accounts(_b: &LiveBackend) -> UiResult<Vec<DebridAccountVm>> {
    Err(UiError::not_wired("debrid_accounts"))
}

/// `Backend::set_debrid_key`.
pub async fn set_debrid_key(_b: &LiveBackend, _service: DebridServiceVm, _key: Option<&str>) -> UiResult<DebridAccountVm> {
    Err(UiError::not_wired("set_debrid_key"))
}

/// `Backend::debrid_play`.
pub async fn debrid_play(_b: &LiveBackend, _source: &str, _id: &str, _episode: u32) -> UiResult<PlaybackVm> {
    Err(UiError::not_wired("debrid_play"))
}

/// Fill `SourceVm::debrid` of the torrent rows from the cache check of every connected service (best effort: on
/// any error the rows stay `None`). Contract stub: returns the list unchanged.
pub async fn mark_cached(_b: &LiveBackend, sources: Vec<SourceVm>) -> Vec<SourceVm> {
    sources
}
