//! `Backend` details extras (wave A, worker **c**; docs/redesign/15 section 4). Bodies: `tk_core::metadata::anilist::details::media_extra` through the AniList provider (`b.router` has no generic call for it: reach AniList directly with the same `reqwest::Client`, or add a `MetadataProvider::extra` default method in tk-core, c's choice, documented in the commit), cached in `b.store` (TTL 6 h); `watch_order` walks `PREQUEL` / `SEQUEL` relations (max 12 hops, cached). Contract stubs.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::{DetailsExtraVm, WatchOrderEntryVm};
use crate::live::LiveBackend;

/// `Backend::details_extra`.
pub async fn details_extra(_b: &LiveBackend, _id: &str) -> UiResult<DetailsExtraVm> {
    Err(UiError::not_wired("details_extra"))
}

/// `Backend::watch_order`.
pub async fn watch_order(_b: &LiveBackend, _id: &str) -> UiResult<Vec<WatchOrderEntryVm>> {
    Err(UiError::not_wired("watch_order"))
}
