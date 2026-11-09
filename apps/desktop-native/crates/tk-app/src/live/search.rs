//! `Backend` search / feed / character calls (wave A, worker **a**; docs/redesign/15-parity-wave-a-contract.md section 4). Bodies: `b.router` + `tk_core::metadata::anilist::search` (AniList only; Kitsu falls back to the plain search), `b.ext` for `search_in_extension` (`search` route), `tk_core::tracemoe::TraceMoe` for images, `b.store` TTL cache for characters. Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::{CharacterVm, ImageMatchVm, ImageQueryVm, MediaVm, SearchPageVm, SearchQueryVm};
use crate::live::LiveBackend;

/// `Backend::search_filtered`.
pub async fn search_filtered(_b: &LiveBackend, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("search_filtered"))
}

/// `Backend::search_in_extension`.
pub async fn search_in_extension(_b: &LiveBackend, _ext_id: &str, _query: &str, _page: u32) -> UiResult<Vec<MediaVm>> {
    Err(UiError::not_wired("search_in_extension"))
}

/// `Backend::feed` (`feed` = `SectionVm::title_key` suffix -> `tk_core::FeedKind`).
pub async fn feed(_b: &LiveBackend, _feed: &str, _page: u32) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("feed"))
}

/// `Backend::search_by_studio`.
pub async fn search_by_studio(_b: &LiveBackend, _studio: &str, _page: u32) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("search_by_studio"))
}

/// `Backend::search_by_image` (trace.moe, then `b.media` for titles).
pub async fn search_by_image(_b: &LiveBackend, _image: ImageQueryVm) -> UiResult<Vec<ImageMatchVm>> {
    Err(UiError::not_wired("search_by_image"))
}

/// `Backend::character`.
pub async fn character(_b: &LiveBackend, _character_id: &str) -> UiResult<CharacterVm> {
    Err(UiError::not_wired("character"))
}
