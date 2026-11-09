//! Social (AniList only; plan 14 section 0.1 #10) preferences and view-models (wave B, worker **n**;
//! docs/redesign/16-parity-wave-b-contract.md section 3.7): profile, followers / following, activity feed,
//! notifications, reviews. `SocialPrefs` is embedded in `Settings::social` (`SettingsAction::SetSocial`). Worker n
//! owns this file; `model/mod.rs` stays frozen. No own backend, no Supabase: everything here is AniList's GraphQL
//! with the token of worker g (`tk_core::secrets::keys::ANILIST_TOKEN`).

use serde::{Deserialize, Serialize};

use crate::backend::MediaVm;

pub const POLL_MAX_MINUTES: u8 = 120;

/// `Settings::social`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SocialPrefs {
    /// How often AniList's unread count is polled for the rail badge; `0` = never (`social.notifications.badge`).
    pub notifications_poll_minutes: u8,
    pub badge: bool,
    /// Show the user's own AniList activity on a title under the details tabs (`components::Reviews`).
    pub show_activity_on_details: bool,
    pub show_reviews_on_details: bool,
}

impl Default for SocialPrefs {
    fn default() -> Self {
        Self { notifications_poll_minutes: 15, badge: true, show_activity_on_details: true, show_reviews_on_details: true }
    }
}

impl SocialPrefs {
    pub fn sanitized(mut self) -> Self {
        self.notifications_poll_minutes = self.notifications_poll_minutes.min(POLL_MAX_MINUTES);
        self
    }
}

/// An AniList user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserVm {
    pub id: u64,
    pub name: String,
    pub avatar: Option<String>,
    /// `https://anilist.co/user/<name>`.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProfileStatsVm {
    pub anime_count: u32,
    pub episodes_watched: u32,
    pub minutes_watched: u64,
    pub manga_count: u32,
    pub chapters_read: u32,
    pub mean_score: Option<f32>,
}

/// `Backend::anilist_profile` (`social.profile.page`, `social.profile.public`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileVm {
    pub user: UserVm,
    pub banner: Option<String>,
    /// AniList markdown, cleaned like synopses.
    pub about: Option<String>,
    pub stats: ProfileStatsVm,
    pub followers: u32,
    pub following: u32,
    /// The viewer follows this user.
    pub is_following: bool,
    /// This user follows the viewer.
    pub is_follower: bool,
    pub is_me: bool,
    pub favorites: Vec<MediaVm>,
    pub created_unix: Option<i64>,
}

/// Which activity feed (`social.activity.feed`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ActivityFeedVm {
    /// Users the viewer follows.
    Following,
    Global,
    User(u64),
    /// Activity about one title (`MediaVm::id`); the details page shows the viewer's own entries.
    Media(String),
}

/// `id()` = `social.activity.kind.<id>`, `data-activity-kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivityKindVm {
    /// Watched / read / plans ... (ListActivity).
    List,
    /// Status post (TextActivity).
    Text,
    /// Message on a profile (MessageActivity).
    Message,
}

impl ActivityKindVm {
    pub const ALL: [ActivityKindVm; 3] = [Self::List, Self::Text, Self::Message];

    pub fn id(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Text => "text",
            Self::Message => "message",
        }
    }
}

/// One activity entry.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivityVm {
    pub id: u64,
    pub kind: ActivityKindVm,
    pub user: UserVm,
    pub media: Option<MediaVm>,
    /// `watched episode`, `read chapter`, `plans to watch` ... (AniList's own wording).
    pub status: Option<String>,
    /// `"12"`, `"1 - 3"`.
    pub progress: Option<String>,
    /// Text / message body (markdown cleaned).
    pub text: Option<String>,
    pub created_unix: i64,
    pub likes: u32,
    pub liked: bool,
    pub replies: u32,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActivityReplyVm {
    pub id: u64,
    pub user: UserVm,
    pub text: String,
    pub created_unix: i64,
    pub likes: u32,
    pub liked: bool,
}

/// One AniList notification (`social.notifications.list`): `kind` is AniList's type name (`AIRING`,
/// `FOLLOWING`, `ACTIVITY_REPLY`, `ACTIVITY_LIKE`, `MEDIA_DATA_CHANGE` ...), `text` display-ready.
#[derive(Debug, Clone, PartialEq)]
pub struct NotificationVm {
    pub id: u64,
    pub kind: String,
    pub text: String,
    pub media: Option<MediaVm>,
    pub user: Option<UserVm>,
    pub created_unix: i64,
    /// Where it leads (a title, a profile, an activity on AniList).
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NotificationPageVm {
    pub items: Vec<NotificationVm>,
    /// AniList's unread count at the time of the request (0 after `anilist_mark_notifications_read`).
    pub unread: u32,
    pub page: u32,
    pub has_next: bool,
}

/// One AniList review of a title (`details.reviews`; `components::Reviews`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewVm {
    pub id: u64,
    pub user: UserVm,
    pub summary: String,
    /// Full body (markdown cleaned), loaded when expanded.
    pub body: Option<String>,
    /// Reviewer's score 0..=100.
    pub score: Option<u32>,
    /// Net up-votes.
    pub rating: i32,
    pub rating_amount: u32,
    pub created_unix: i64,
    pub url: String,
}

/// A page of users (followers / following).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct UserPageVm {
    pub items: Vec<UserVm>,
    pub page: u32,
    pub has_next: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = SocialPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(SocialPrefs { notifications_poll_minutes: 255, ..d }.sanitized().notifications_poll_minutes, POLL_MAX_MINUTES);
    }
}
