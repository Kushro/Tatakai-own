//! Manga + the reading calls shared with novels (wave B, worker **l**; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `manga.*`). Contract stubs: l replaces every body. tk-core side: `tk_core::manga`
//! (chapter list merge per provider, best-source pick), `tk_core::metadata::{mangadex, mangaupdates}` and
//! `tk_core::metadata::anilist::manga` (AniList `MANGA` search / details / list), `LocalStore` tables of
//! `tk-core/src/store/reading.rs` (`read_progress`, `chapter_lists`, `offline_chapters`, `read_list_entries`,
//! `read_list_queue`). Chapters come from installed extensions with the `manga` capability (`tk_ext::ExtensionHost::call`
//! with `chapters` / `pages`) and from enabled extension servers (`~read` variants, worker j's client).
//! `read_*` take `ReadKindVm` so worker m reuses them for novels without copying.

use tk_ui::backend::{
    ChapterProviderVm, ChapterRefVm, ListEntryUpdateVm, ListEntryVm, ListStatusVm, MangaDetailsVm, PageVm, ReadHomeVm,
    ReadProgressVm, SearchPageVm, SearchQueryVm, UiResult,
};
use tk_ui::model::{ReadKindVm, UiError};

use crate::live::LiveBackend;

pub async fn home(_b: &LiveBackend) -> UiResult<ReadHomeVm> {
    Err(UiError::not_wired("manga_home"))
}
pub async fn search(_b: &LiveBackend, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("manga_search"))
}
pub async fn details(_b: &LiveBackend, _id: &str) -> UiResult<MangaDetailsVm> {
    Err(UiError::not_wired("manga_details"))
}
pub async fn chapters(_b: &LiveBackend, _id: &str, _refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
    Err(UiError::not_wired("manga_chapters"))
}
pub async fn pages(_b: &LiveBackend, _chapter: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
    Err(UiError::not_wired("manga_pages"))
}
pub async fn read_progress(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str) -> UiResult<Vec<ReadProgressVm>> {
    Err(UiError::not_wired("read_progress"))
}
pub async fn set_read_progress(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str, _progress: &ReadProgressVm) -> UiResult<()> {
    Err(UiError::not_wired("set_read_progress"))
}
pub async fn read_list_entry(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str) -> UiResult<Option<ListEntryVm>> {
    Err(UiError::not_wired("read_list_entry"))
}
pub async fn set_read_list_entry(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str, _update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
    Err(UiError::not_wired("set_read_list_entry"))
}
pub async fn remove_read_list_entry(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str) -> UiResult<()> {
    Err(UiError::not_wired("remove_read_list_entry"))
}
pub async fn read_list_entries(_b: &LiveBackend, _kind: ReadKindVm, _status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
    Err(UiError::not_wired("read_list_entries"))
}
pub async fn download_chapters(_b: &LiveBackend, _id: &str, _chapters: &[ChapterRefVm]) -> UiResult<u32> {
    Err(UiError::not_wired("manga_download_chapters"))
}
pub async fn remove_offline(_b: &LiveBackend, _kind: ReadKindVm, _chapter: &ChapterRefVm) -> UiResult<()> {
    Err(UiError::not_wired("read_remove_offline"))
}
pub async fn clear_cache(_b: &LiveBackend, _kind: ReadKindVm, _media_id: &str) -> UiResult<()> {
    Err(UiError::not_wired("read_clear_cache"))
}
