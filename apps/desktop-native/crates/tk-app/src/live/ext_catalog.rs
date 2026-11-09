//! Stremio add-ons, extension updates, per-extension preferences, repositories and custom providers (wave B, worker
//! **j**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist `ext.stremio*`, `ext.updates.*`,
//! `ext.preferences`, `ext.repositories`, `ext.install-url`, `ext.kill-switch`, `ext.custom-providers.*`). Contract
//! stubs: j replaces every body. tk-core side: `tk_core::stremio` (manifest + `/stream/<type>/<id>.json`),
//! `LocalStore` tables of `tk-core/src/store/ext.rs` (`stremio_addons`, `ext_repos`, `ext_prefs`, `ext_updates`,
//! `provider_bookmarks`); the extension host (`b.ext.host()`, `tk_ext::ExtensionHost::call` with the `custom/home`,
//! `custom/search`, `custom/info`, `custom/watch`, `custom/read` routes).
//!
//! `LiveBackend` routes every `ext:<ext_id>:<key>` id (`is_provider_id`) of `details`, `episodes`, `playback`,
//! `sources` and `record_progress` here, so provider titles work in the normal Details page and player
//! (`ext.custom-providers.info` / `.watch` / `.continue`; KNOWN-ISSUES of wave A).

use tk_core::sources::SourceCandidate;
use tk_core::Media;
use tk_ui::backend::{
    CustomProviderVm, DetailsVm, EpisodeVm, ExtCatalogItemVm, ExtPrefFieldVm, ExtRepoVm, ExtUpdateVm, ExtensionVm, MediaVm,
    PlaybackVm, ProviderItemVm, SearchPageVm, SectionVm, SourceVm, StremioAddonVm, UiResult,
};
use tk_ui::model::UiError;

use crate::live::LiveBackend;

/// `ext:<extension id>:<key>`: a title that only an extension knows (Search "in extension", provider pages).
pub fn is_provider_id(id: &str) -> bool {
    id.starts_with("ext:")
}

/// Candidates from paired extension servers (`ext.server-client.sources`, `ext.ecosystems.via-server`) and Stremio
/// add-ons (`ext.stremio`) for the ranking of `LiveBackend::episode_candidates`. `ext_id` of a remote candidate is
/// `server:<server id>` (`label` = the server's source name); of a Stremio one `stremio:<addon id>`. Empty until j lands.
pub async fn extra_candidates(_b: &LiveBackend, _media: &Media, _episode: u32) -> Vec<SourceCandidate> {
    Vec::new()
}

pub async fn details(_b: &LiveBackend, id: &str) -> UiResult<DetailsVm> {
    Err(UiError::not_wired(&format!("details({id})")))
}
pub async fn episodes(_b: &LiveBackend, id: &str) -> UiResult<Vec<EpisodeVm>> {
    Err(UiError::not_wired(&format!("episodes({id})")))
}
pub async fn playback(_b: &LiveBackend, id: &str, _episode: u32) -> UiResult<PlaybackVm> {
    Err(UiError::not_wired(&format!("playback({id})")))
}
pub async fn sources(_b: &LiveBackend, id: &str, _episode: u32) -> UiResult<Vec<SourceVm>> {
    Err(UiError::not_wired(&format!("sources({id})")))
}
/// History rows of provider titles (`ext.custom-providers.continue`): `HistoryEntry` with the `ext:` id as `anime_id`
/// and no AniList / MAL ids (local only; Link skips rows without them, which is intended).
pub async fn record_progress(_b: &LiveBackend, id: &str, _episode: u32, _progress_secs: u32, _duration_secs: Option<u32>) -> UiResult<()> {
    Err(UiError::not_wired(&format!("record_progress({id})")))
}

pub async fn stremio_addons(_b: &LiveBackend) -> UiResult<Vec<StremioAddonVm>> {
    Err(UiError::not_wired("stremio_addons"))
}
pub async fn stremio_add(_b: &LiveBackend, _manifest_url: &str) -> UiResult<StremioAddonVm> {
    Err(UiError::not_wired("stremio_add"))
}
pub async fn stremio_remove(_b: &LiveBackend, _id: &str) -> UiResult<()> {
    Err(UiError::not_wired("stremio_remove"))
}
pub async fn stremio_set_enabled(_b: &LiveBackend, _id: &str, _enabled: bool) -> UiResult<()> {
    Err(UiError::not_wired("stremio_set_enabled"))
}
pub async fn updates_check(_b: &LiveBackend) -> UiResult<Vec<ExtUpdateVm>> {
    Err(UiError::not_wired("ext_updates_check"))
}
pub async fn update(_b: &LiveBackend, _id: &str) -> UiResult<ExtensionVm> {
    Err(UiError::not_wired("ext_update"))
}
pub async fn update_all(_b: &LiveBackend) -> UiResult<u32> {
    Err(UiError::not_wired("ext_update_all"))
}
pub async fn set_all_enabled(_b: &LiveBackend, _enabled: bool) -> UiResult<u32> {
    Err(UiError::not_wired("set_all_extensions_enabled"))
}
pub async fn install_url(_b: &LiveBackend, _url: &str) -> UiResult<ExtensionVm> {
    Err(UiError::not_wired("install_extension_url"))
}
pub async fn preferences(_b: &LiveBackend, _id: &str) -> UiResult<Vec<ExtPrefFieldVm>> {
    Err(UiError::not_wired("ext_preferences"))
}
pub async fn set_preference(_b: &LiveBackend, _id: &str, _key: &str, _value: &str) -> UiResult<()> {
    Err(UiError::not_wired("set_ext_preference"))
}
pub async fn repos(_b: &LiveBackend) -> UiResult<Vec<ExtRepoVm>> {
    Err(UiError::not_wired("ext_repos"))
}
pub async fn repo_add(_b: &LiveBackend, _url: &str) -> UiResult<ExtRepoVm> {
    Err(UiError::not_wired("ext_repo_add"))
}
pub async fn repo_remove(_b: &LiveBackend, _id: &str) -> UiResult<()> {
    Err(UiError::not_wired("ext_repo_remove"))
}
pub async fn repo_catalog(_b: &LiveBackend, _id: &str, _refresh: bool) -> UiResult<Vec<ExtCatalogItemVm>> {
    Err(UiError::not_wired("ext_repo_catalog"))
}
pub async fn custom_providers(_b: &LiveBackend) -> UiResult<Vec<CustomProviderVm>> {
    Err(UiError::not_wired("custom_providers"))
}
pub async fn provider_home(_b: &LiveBackend, _ext_id: &str) -> UiResult<Vec<SectionVm>> {
    Err(UiError::not_wired("provider_home"))
}
pub async fn provider_feed(_b: &LiveBackend, _ext_id: &str, _row: &str, _page: u32) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("provider_feed"))
}
pub async fn provider_item(_b: &LiveBackend, _ext_id: &str, _item_key: &str) -> UiResult<ProviderItemVm> {
    Err(UiError::not_wired("provider_item"))
}
pub async fn provider_bookmarks(_b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Err(UiError::not_wired("provider_bookmarks"))
}
pub async fn set_provider_bookmark(_b: &LiveBackend, _ext_id: &str, _item_key: &str, _bookmarked: bool) -> UiResult<()> {
    Err(UiError::not_wired("set_provider_bookmark"))
}

#[cfg(test)]
mod tests {
    use super::is_provider_id;

    #[test]
    fn provider_ids_have_the_ext_prefix() {
        assert!(is_provider_id("ext:demo:src:42"));
        assert!(!is_provider_id("21"));
        assert!(!is_provider_id("mal:1"));
    }
}
