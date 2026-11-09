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

// ----- helpers shared by tk-app (`live/social.rs`) and the social screens (worker n, private to this area) -----

/// `MediaVm::id` prefix of an AniList manga in social view-models (favourites, activity, notifications): the
/// social pages open `/manga/<anilist id>` for it. Anime keep the usual UI id (`"21"`).
pub const MANGA_PREFIX: &str = "manga:";
/// Same for AniList light novels (`format: NOVEL`): `/novels/<anilist id>`.
pub const NOVEL_PREFIX: &str = "novel:";

/// Choices of Settings > Social > "Check notifications every" (minutes; `0` = never).
pub const POLL_CHOICES: [u8; 6] = [0, 5, 15, 30, 60, POLL_MAX_MINUTES];

/// What a social `MediaVm::id` points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocialTarget<'a> {
    Anime(&'a str),
    Manga(&'a str),
    Novel(&'a str),
}

/// Split a social media id into its page and the id that page takes.
pub fn social_target(id: &str) -> SocialTarget<'_> {
    if let Some(rest) = id.strip_prefix(MANGA_PREFIX) {
        SocialTarget::Manga(rest)
    } else if let Some(rest) = id.strip_prefix(NOVEL_PREFIX) {
        SocialTarget::Novel(rest)
    } else {
        SocialTarget::Anime(id)
    }
}

/// Filter groups of the notifications page (`social.notifications.group.<id>`, `data-group`).
pub const NOTIFICATION_GROUPS: [&str; 6] = ["all", "airing", "follows", "activity", "forum", "media"];

/// Group of an AniList `NotificationType`.
pub fn notification_group(kind: &str) -> &'static str {
    match kind {
        "AIRING" => "airing",
        "FOLLOWING" => "follows",
        k if k.starts_with("ACTIVITY_") => "activity",
        k if k.starts_with("THREAD_") => "forum",
        _ => "media",
    }
}

/// Localized sentence of a notification (`social.notif.<type in lowercase>`), falling back to AniList's own
/// English sentence when the type is unknown or a needed name is missing. Placeholders: `{user}`, `{media}`,
/// `{episode}`, `{thread}`.
pub fn notification_text(
    kind: &str,
    user: Option<&str>,
    media: Option<&str>,
    episode: Option<u32>,
    thread: Option<&str>,
    fallback: &str,
) -> String {
    let key = format!("social.notif.{}", kind.to_ascii_lowercase());
    let template = crate::i18n::lookup(crate::i18n::lang(), &key);
    if template == key {
        return fallback.to_owned();
    }
    let needs = |p: &str| template.contains(p);
    if (needs("{user}") && user.is_none())
        || (needs("{media}") && media.is_none())
        || (needs("{episode}") && episode.is_none())
        || (needs("{thread}") && thread.is_none())
    {
        return fallback.to_owned();
    }
    let episode = episode.map(|e| e.to_string()).unwrap_or_default();
    template
        .replace("{user}", user.unwrap_or_default())
        .replace("{media}", media.unwrap_or_default())
        .replace("{episode}", &episode)
        .replace("{thread}", thread.unwrap_or_default())
}

/// Day groups of the notifications list (`social.notifications.day.<id>`), by age.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DayBucket {
    Today,
    Yesterday,
    Week,
    Month,
    Older,
}

impl DayBucket {
    pub fn id(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Yesterday => "yesterday",
            Self::Week => "week",
            Self::Month => "month",
            Self::Older => "older",
        }
    }

    /// Bucket of something created at `then` seen at `now` (unix seconds; future dates count as today).
    pub fn of(now: i64, then: i64) -> Self {
        let age = (now - then).max(0);
        match age / 86_400 {
            0 => Self::Today,
            1 => Self::Yesterday,
            2..=6 => Self::Week,
            7..=29 => Self::Month,
            _ => Self::Older,
        }
    }
}

/// Consecutive runs of items that share a bucket, in input order (the list is newest first).
pub fn group_by_day<T: Clone>(now: i64, items: &[T], created: impl Fn(&T) -> i64) -> Vec<(DayBucket, Vec<T>)> {
    let mut out: Vec<(DayBucket, Vec<T>)> = Vec::new();
    for it in items {
        let b = DayBucket::of(now, created(it));
        match out.last_mut() {
            Some((last, list)) if *last == b => list.push(it.clone()),
            _ => out.push((b, vec![it.clone()])),
        }
    }
    out
}

/// AniList's notifications come newest first and the first `unread` of them are the unread ones.
pub fn is_unread(index: usize, unread: u32) -> bool {
    index < unread as usize
}

/// The part of one notifications page that is unread (`unread_only`): `offset` = items on the earlier pages.
pub fn unread_part<T>(items: Vec<T>, offset: usize, unread: u32) -> Vec<T> {
    items.into_iter().enumerate().filter(|(i, _)| is_unread(offset + i, unread)).map(|(_, x)| x).collect()
}

/// Hours watched (rounded down) from minutes.
pub fn hours_watched(minutes: u64) -> u64 {
    minutes / 60
}

/// Wakes the rail badge poll (`components::rail_badges`) after "Mark all read" or when the window regains focus.
static UNREAD_WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();

pub fn wake_unread_poll() {
    UNREAD_WAKE.notify_one();
}

/// Sleep `minutes` (at least one) or until [`wake_unread_poll`], whichever comes first. The rail badge loop uses it
/// in place of a plain sleep so the count refreshes at once after reading the notifications.
pub async fn wait_unread_poll(minutes: u8) {
    let d = std::time::Duration::from_secs(60 * u64::from(minutes.max(1)));
    let _ = tokio::time::timeout(d, UNREAD_WAKE.notified()).await;
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
    #[test]
    fn social_targets() {
        assert_eq!(social_target("21"), SocialTarget::Anime("21"));
        assert_eq!(social_target("mal:5"), SocialTarget::Anime("mal:5"));
        assert_eq!(social_target("manga:30013"), SocialTarget::Manga("30013"));
        assert_eq!(social_target("novel:86300"), SocialTarget::Novel("86300"));
    }

    #[test]
    fn notification_groups_cover_every_type() {
        assert_eq!(notification_group("AIRING"), "airing");
        assert_eq!(notification_group("FOLLOWING"), "follows");
        assert_eq!(notification_group("ACTIVITY_REPLY_LIKE"), "activity");
        assert_eq!(notification_group("THREAD_LIKE"), "forum");
        assert_eq!(notification_group("MEDIA_MERGE"), "media");
        for g in NOTIFICATION_GROUPS {
            assert!(!g.is_empty());
        }
    }

    #[test]
    fn notification_text_falls_back() {
        let follow = notification_text("FOLLOWING", Some("Mia"), None, None, None, "x");
        assert!(follow.contains("Mia") && !follow.contains('{'), "{follow}");
        assert_eq!(notification_text("FOLLOWING", None, None, None, None, "AniList says"), "AniList says");
        assert_eq!(notification_text("SOMETHING_NEW", Some("a"), None, None, None, "fallback"), "fallback");
        let airing = notification_text("AIRING", None, Some("Frieren"), Some(5), None, "x");
        assert!(airing.contains("Frieren") && airing.contains('5'), "{airing}");
        assert_eq!(notification_text("AIRING", None, Some("Frieren"), None, None, "fb"), "fb");
    }

    #[test]
    fn day_buckets_and_grouping() {
        let now = 10 * 86_400;
        assert_eq!(DayBucket::of(now, now + 50), DayBucket::Today);
        assert_eq!(DayBucket::of(now, now - 3_600), DayBucket::Today);
        assert_eq!(DayBucket::of(now, now - 86_400), DayBucket::Yesterday);
        assert_eq!(DayBucket::of(now, now - 3 * 86_400), DayBucket::Week);
        assert_eq!(DayBucket::of(now, now - 8 * 86_400), DayBucket::Month);
        assert_eq!(DayBucket::of(now, 0), DayBucket::Month);
        assert_eq!(DayBucket::of(40 * 86_400, 0), DayBucket::Older);
        let items = vec![now, now - 10, now - 86_400, now - 86_401, now - 9 * 86_400];
        let g = group_by_day(now, &items, |t| *t);
        assert_eq!(g.iter().map(|(b, l)| (b.id(), l.len())).collect::<Vec<_>>(), vec![("today", 2), ("yesterday", 2), ("month", 1)]);
    }

    #[test]
    fn unread_flags_follow_the_count() {
        assert!(is_unread(0, 2) && is_unread(1, 2) && !is_unread(2, 2));
        assert!(!is_unread(0, 0));
        assert_eq!(unread_part(vec!['a', 'b', 'c'], 0, 2), vec!['a', 'b']);
        assert_eq!(unread_part(vec!['d', 'e'], 25, 26), vec!['d']);
        assert!(unread_part(vec!['f'], 25, 3).is_empty());
        assert_eq!(hours_watched(125), 2);
        assert!(POLL_CHOICES.iter().all(|m| *m <= POLL_MAX_MINUTES));
        assert!(POLL_CHOICES.contains(&SocialPrefs::default().notifications_poll_minutes));
    }
    // ---- F6 (parity wave B) ----

    #[test]
    fn every_poll_choice_survives_sanitizing_and_booleans_are_kept() {
        for m in POLL_CHOICES {
            let p = SocialPrefs { notifications_poll_minutes: m, ..SocialPrefs::default() };
            assert_eq!(p.clone().sanitized(), p, "{m}");
        }
        let off = SocialPrefs { notifications_poll_minutes: 0, badge: false, show_activity_on_details: false, show_reviews_on_details: false };
        assert_eq!(off.clone().sanitized(), off);
        let p: SocialPrefs = serde_json::from_str(r#"{"badge":false}"#).unwrap();
        assert_eq!(p, SocialPrefs { badge: false, ..SocialPrefs::default() });
    }

    #[test]
    fn day_bucket_edges() {
        let now = 100 * 86_400;
        assert_eq!(DayBucket::of(now, now - 86_399), DayBucket::Today);
        assert_eq!(DayBucket::of(now, now - 2 * 86_400 + 1), DayBucket::Yesterday);
        assert_eq!(DayBucket::of(now, now - 7 * 86_400 + 1), DayBucket::Week);
        assert_eq!(DayBucket::of(now, now - 7 * 86_400), DayBucket::Month);
        assert_eq!(DayBucket::of(now, now - 30 * 86_400), DayBucket::Older);
        assert!(group_by_day(now, &Vec::<i64>::new(), |t| *t).is_empty());
        // runs are consecutive: the same bucket after another one opens a new group
        let g = group_by_day(now, &[now, now - 86_400, now], |t| *t);
        assert_eq!(g.len(), 3);
    }

    #[test]
    fn prefixes_strip_only_at_the_start() {
        assert_eq!(social_target("xmanga:1"), SocialTarget::Anime("xmanga:1"));
        assert_eq!(social_target("manga:"), SocialTarget::Manga(""));
        assert_eq!(notification_group(""), "media");
        assert_eq!(notification_group("activity_like"), "media", "AniList types are upper case");
        assert_eq!(hours_watched(59), 0);
    }
}
