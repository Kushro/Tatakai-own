//! AniList social (wave B, worker **n**; plan 14 section 0.1 #10; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `social.*`, `details.reviews`). Contract stubs: n replaces every body. All of it is AniList
//! GraphQL (`tk_core::metadata::anilist::social`: `Viewer`, `User(name)`, `Page.followers / following`,
//! `Page.activities`, `ToggleFollow`, `ToggleLikeV2`, `SaveActivityReply`, `SaveTextActivity`, `DeleteActivity`,
//! `Page.notifications(resetNotificationCount)`, `Viewer.unreadNotificationCount`, `Page.reviews(mediaId)`,
//! `Review(id)`) with the token worker g stores (`b.secrets.get(tk_core::secrets::keys::ANILIST_TOKEN)`); without a
//! token every call answers `ErrorKind::Auth` (the UI shows "connect AniList"). Short-lived cache in
//! `tk-core/src/store/social.rs` (`social_cache`, TTL minutes) so the rail badge and Details do not hammer AniList
//! (90 req/min).

use tk_ui::backend::{
    ActivityFeedVm, ActivityReplyVm, ActivityVm, NotificationPageVm, ProfileVm, ReviewVm, UiResult, UserPageVm,
};
use tk_ui::model::UiError;

use crate::live::LiveBackend;

pub async fn profile(_b: &LiveBackend, _name: Option<&str>) -> UiResult<ProfileVm> {
    Err(UiError::not_wired("anilist_profile"))
}
pub async fn follow(_b: &LiveBackend, _user_id: u64, _follow: bool) -> UiResult<()> {
    Err(UiError::not_wired("anilist_follow"))
}
pub async fn followers(_b: &LiveBackend, _user_id: u64, _page: u32) -> UiResult<UserPageVm> {
    Err(UiError::not_wired("anilist_followers"))
}
pub async fn following(_b: &LiveBackend, _user_id: u64, _page: u32) -> UiResult<UserPageVm> {
    Err(UiError::not_wired("anilist_following"))
}
pub async fn activity(_b: &LiveBackend, _feed: &ActivityFeedVm, _page: u32) -> UiResult<Vec<ActivityVm>> {
    Err(UiError::not_wired("anilist_activity"))
}
pub async fn activity_replies(_b: &LiveBackend, _activity_id: u64) -> UiResult<Vec<ActivityReplyVm>> {
    Err(UiError::not_wired("anilist_activity_replies"))
}
pub async fn like_activity(_b: &LiveBackend, _activity_id: u64, _like: bool) -> UiResult<()> {
    Err(UiError::not_wired("anilist_like_activity"))
}
pub async fn reply(_b: &LiveBackend, _activity_id: u64, _text: &str) -> UiResult<ActivityReplyVm> {
    Err(UiError::not_wired("anilist_reply"))
}
pub async fn post_status(_b: &LiveBackend, _text: &str) -> UiResult<ActivityVm> {
    Err(UiError::not_wired("anilist_post_status"))
}
pub async fn delete_activity(_b: &LiveBackend, _activity_id: u64) -> UiResult<()> {
    Err(UiError::not_wired("anilist_delete_activity"))
}
pub async fn notifications(_b: &LiveBackend, _page: u32, _unread_only: bool) -> UiResult<NotificationPageVm> {
    Err(UiError::not_wired("anilist_notifications"))
}
/// `Backend::anilist_unread_count`: polled by the rail badge; n caches it for `Settings::social.notifications_poll_minutes`.
pub async fn unread_count(_b: &LiveBackend) -> UiResult<u32> {
    Err(UiError::not_wired("anilist_unread_count"))
}
pub async fn mark_notifications_read(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("anilist_mark_notifications_read"))
}
pub async fn reviews(_b: &LiveBackend, _media_id: &str, _page: u32) -> UiResult<Vec<ReviewVm>> {
    Err(UiError::not_wired("anilist_reviews"))
}
pub async fn review(_b: &LiveBackend, _review_id: u64) -> UiResult<ReviewVm> {
    Err(UiError::not_wired("anilist_review"))
}
