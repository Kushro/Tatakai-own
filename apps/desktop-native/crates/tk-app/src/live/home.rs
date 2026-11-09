//! Home, discover, airing calendar, recommendations and metadata-provider health (wave B, worker **b**;
//! docs/redesign/16-parity-wave-b-contract.md section 4; checklist `home.*`, `details.track-show`,
//! `settings.metadata.extra-providers`, `settings.metadata.provider-status`, `metadata.providers.*`).
//!
//! `home` is the wave-0 body moved here verbatim so b extends it (latest episodes, Top 10, upcoming, most favourite,
//! "because you watched", genre rows, followed updates, torrent activity) without touching `live/mod.rs`. Everything
//! else is a contract stub (`NotWired`) that b replaces. tk-core side: `tk_core::recs` (taste profile, atlas
//! projection), `tk_core::metadata::anilist::airing` (airing schedule, genres, top ranges), `tk_core::metadata::{mal,
//! shikimori, tenrai}` (providers registered by `MetadataRouter::with_defaults`), `LocalStore` tables of
//! `tk-core/src/store/home.rs` (`tracked_shows`, `recs_feedback`, `home_visits`).

use tk_core::{FeedKind, MediaKind, ProviderError};
use tk_ui::backend::{
    AiringSlotVm, AtlasPointVm, BecauseVm, HomeVm, LatestEpisodeVm, MediaVm, ProviderHealthVm, RecommendationVm,
    RecsFeedbackVm, SearchPageVm, SectionVm, TasteProfileVm, TopRangeVm, TorrentActivityVm, UiResult,
};
use tk_ui::model::UiError;

use crate::live::{LiveBackend, provider_err, to_vm};

/// `Backend::home` (wave 0 body; b extends it with the wave B rows behind `Settings::home` toggles).
pub async fn home(b: &LiveBackend) -> UiResult<HomeVm> {
    let feed = |f| b.router.feed(MediaKind::Anime, f, 1);
    let (trending, airing, seasonal, popular, top) = tokio::join!(
        feed(FeedKind::Trending),
        feed(FeedKind::Airing),
        feed(FeedKind::Seasonal),
        feed(FeedKind::Popular),
        feed(FeedKind::TopRated)
    );
    let section = |key: &str, r: Result<tk_core::FeedPage, ProviderError>| SectionVm {
        title_key: key.to_owned(),
        items: r.map(|p| p.items.iter().map(to_vm).collect()).map_err(provider_err),
    };
    let continue_watching = b.continue_vms(20);
    Ok(HomeVm {
        continue_watching,
        sections: vec![
            section("feed.trending", trending),
            section("feed.airing", airing),
            section("feed.seasonal", seasonal),
            section("feed.popular", popular),
            section("feed.top_rated", top),
        ],
    })
}

pub async fn latest_episodes(_b: &LiveBackend, _page: u32) -> UiResult<Vec<LatestEpisodeVm>> {
    Err(UiError::not_wired("latest_episodes"))
}
pub async fn top10(_b: &LiveBackend, _range: TopRangeVm) -> UiResult<Vec<MediaVm>> {
    Err(UiError::not_wired("top10"))
}
pub async fn because_you_watched(_b: &LiveBackend) -> UiResult<Vec<BecauseVm>> {
    Err(UiError::not_wired("because_you_watched"))
}
pub async fn genres(_b: &LiveBackend) -> UiResult<Vec<String>> {
    Err(UiError::not_wired("genres"))
}
pub async fn genre_feed(_b: &LiveBackend, _genre: &str, _page: u32) -> UiResult<SearchPageVm> {
    Err(UiError::not_wired("genre_feed"))
}
pub async fn followed_updates(_b: &LiveBackend) -> UiResult<Vec<LatestEpisodeVm>> {
    Err(UiError::not_wired("followed_updates"))
}
pub async fn airing_schedule(_b: &LiveBackend, _week_start_unix: i64) -> UiResult<Vec<AiringSlotVm>> {
    Err(UiError::not_wired("airing_schedule"))
}
pub async fn tracked_shows(_b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Err(UiError::not_wired("tracked_shows"))
}
pub async fn is_tracked(_b: &LiveBackend, _media_id: &str) -> UiResult<bool> {
    Err(UiError::not_wired("is_tracked"))
}
pub async fn set_tracked(_b: &LiveBackend, _media_id: &str, _tracked: bool) -> UiResult<()> {
    Err(UiError::not_wired("set_tracked"))
}
pub async fn reminders_due(_b: &LiveBackend) -> UiResult<Vec<AiringSlotVm>> {
    Err(UiError::not_wired("reminders_due"))
}
pub async fn recommendations(_b: &LiveBackend, _page: u32) -> UiResult<Vec<RecommendationVm>> {
    Err(UiError::not_wired("recommendations"))
}
pub async fn recs_feedback(_b: &LiveBackend, _media_id: &str, _feedback: RecsFeedbackVm) -> UiResult<()> {
    Err(UiError::not_wired("recs_feedback"))
}
pub async fn taste_profile(_b: &LiveBackend) -> UiResult<TasteProfileVm> {
    Err(UiError::not_wired("taste_profile"))
}
pub async fn atlas_points(_b: &LiveBackend) -> UiResult<Vec<AtlasPointVm>> {
    Err(UiError::not_wired("atlas_points"))
}
/// `Backend::torrent_activity`: b builds it from `b.torrent.library()` (worker i's engine, wave A).
pub async fn torrent_activity(_b: &LiveBackend) -> UiResult<TorrentActivityVm> {
    Err(UiError::not_wired("torrent_activity"))
}
/// `Backend::provider_health`: `b.router.cooling_down()` + the per-provider last result b records in `tk_core::metadata`.
pub async fn provider_health(_b: &LiveBackend) -> UiResult<Vec<ProviderHealthVm>> {
    Err(UiError::not_wired("provider_health"))
}
