//! AniList social queries (wave B, worker **n**; plan 14 section 0.1 #10; checklist `social.*`, `details.reviews`).
//! Child module of `anilist`: every request goes through [`AniList::post_auth`] (bearer token from `Secrets`, read
//! by tk-app; public reads such as reviews and public profiles also work without one).
//!
//! - Profiles: [`AniList::viewer_profile`] (`Viewer`) and [`AniList::user_profile`] (`User(name:)`) with
//!   `about`, `bannerImage`, `statistics { anime manga }`, `favourites { anime manga }`, `isFollowing`,
//!   `isFollower`; follower / following totals from `Page.followers / following` `pageInfo.total`.
//! - Follows: [`AniList::set_follow`] (`ToggleFollow`, only when the state differs), [`AniList::followers`] /
//!   [`AniList::following`] (paginated).
//! - Activity: [`AniList::activities`] (`Page.activities` with the `ListActivity` / `TextActivity` /
//!   `MessageActivity` fragments), [`AniList::activity_replies`], [`AniList::set_activity_like`]
//!   (`ToggleLikeV2(type: ACTIVITY)`, only when the state differs), [`AniList::save_reply`]
//!   (`SaveActivityReply`), [`AniList::save_text_activity`] (`SaveTextActivity`), [`AniList::delete_activity`].
//! - Notifications: [`AniList::notifications`] (`Page.notifications(resetNotificationCount:)` with every
//!   notification type fragment), [`AniList::unread_count`] (`Viewer.unreadNotificationCount`),
//!   [`AniList::reset_notifications`].
//! - Reviews: [`AniList::reviews`] (`Media.reviews(sort: RATING_DESC)` plus `averageScore`), [`AniList::review`]
//!   (`Review { body(asHtml: false) }`).
//!
//! Every JSON -> struct mapper is a pure function with unit tests; the structs are `Serialize` so tk-app can keep
//! them in `social_cache`. AniList markdown (`img220(...)`, `~!spoiler!~`, `__bold__`, `[text](url)`, HTML) is
//! flattened to plain text by [`clean_markdown`].

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{AniList, CARD_FRAGMENT};
use crate::error::ListError;
use crate::metadata::mapping::{clean_synopsis, map_anilist_media};
use crate::model::Media;

/// Followers / following / activity page size.
pub const USERS_PER_PAGE: u32 = 50;
pub const ACTIVITIES_PER_PAGE: u32 = 25;
pub const NOTIFICATIONS_PER_PAGE: u32 = 25;
pub const REVIEWS_PER_PAGE: u32 = 10;
/// AniList rejects longer text activities / replies.
pub const MAX_TEXT_CHARS: usize = 2000;

/// An AniList user (`id name avatar siteUrl`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocialUser {
    pub id: u64,
    pub name: String,
    pub avatar: Option<String>,
    pub site_url: String,
}

/// A title referenced by a social object; `format` tells manga from light novels (`NOVEL`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SocialMedia {
    pub media: Media,
    pub format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UserStats {
    pub anime_count: u32,
    pub episodes_watched: u32,
    pub minutes_watched: u64,
    pub manga_count: u32,
    pub chapters_read: u32,
    /// Anime mean score 0..=100 (`statistics.anime.meanScore`).
    pub mean_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub user: SocialUser,
    pub banner: Option<String>,
    pub about: Option<String>,
    pub stats: UserStats,
    pub followers: u32,
    pub following: u32,
    pub is_following: bool,
    pub is_follower: bool,
    /// Favourite anime first, then manga.
    pub favourites: Vec<SocialMedia>,
    pub created_at: Option<i64>,
}

/// A page of users.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UserPage {
    pub items: Vec<SocialUser>,
    pub page: u32,
    pub has_next: bool,
}

/// Which activities to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityFeed {
    /// The viewer and the users they follow.
    Following,
    /// Everyone (`hasRepliesOrTypeText`: AniList's own global feed without the list-progress noise).
    Global,
    User(u64),
    /// One title, optionally one user's activity on it (the details page shows the viewer's own).
    Media {
        media_id: u32,
        user_id: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    List,
    Text,
    Message,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Activity {
    pub id: u64,
    pub kind: ActivityKind,
    /// Author (the messenger for a message).
    pub user: SocialUser,
    pub media: Option<SocialMedia>,
    pub status: Option<String>,
    pub progress: Option<String>,
    pub text: Option<String>,
    pub created_at: i64,
    pub likes: u32,
    pub liked: bool,
    pub replies: u32,
    pub site_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityReply {
    pub id: u64,
    pub user: SocialUser,
    pub text: String,
    pub created_at: i64,
    pub likes: u32,
    pub liked: bool,
}

/// One notification; `kind` is AniList's `NotificationType` (`AIRING`, `FOLLOWING`, `ACTIVITY_LIKE` ...).
/// `context` is AniList's own English sentence fragment(s) (fallback text); the UI builds its localized text from
/// `kind`, `user`, `media`, `episode` and `thread_title`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    pub id: u64,
    pub kind: String,
    pub user: Option<SocialUser>,
    pub media: Option<SocialMedia>,
    pub episode: Option<u32>,
    pub activity_id: Option<u64>,
    pub thread_title: Option<String>,
    /// Where it leads on anilist.co (activity, forum thread, media, profile).
    pub url: Option<String>,
    /// AniList's sentence, already assembled (`"Episode 5 of X aired."`, `"Y started following you."`).
    pub context: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct NotificationPage {
    pub items: Vec<Notification>,
    pub page: u32,
    pub has_next: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub id: u64,
    pub user: SocialUser,
    pub summary: String,
    pub body: Option<String>,
    pub score: Option<u32>,
    pub rating: i32,
    pub rating_amount: u32,
    pub created_at: i64,
    pub site_url: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ReviewPage {
    pub items: Vec<Review>,
    pub page: u32,
    pub has_next: bool,
    /// The title's AniList id (resolved from a MAL id when needed).
    pub media_id: Option<u32>,
    /// `Media.averageScore` 0..=100.
    pub average_score: Option<u32>,
}

/// How the caller names a title (`MediaVm::id` is an AniList id or `mal:<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    AniList(u32),
    Mal(u32),
}

// ----- queries -----

const USER_FIELDS: &str = "id name avatar { large medium } siteUrl";

const PROFILE_FIELDS: &str = "id name about(asHtml: false) avatar { large medium } bannerImage siteUrl createdAt isFollowing \
isFollower statistics { anime { count episodesWatched minutesWatched meanScore } manga { count chaptersRead } } \
favourites { anime(perPage: 12) { nodes { ...mediaCard } } manga(perPage: 12) { nodes { ...mediaCard } } }";

fn viewer_profile_query() -> String {
    format!("query {{ Viewer {{ {PROFILE_FIELDS} }} }} {CARD_FRAGMENT}")
}

fn user_profile_query() -> String {
    format!("query ($name: String) {{ User(name: $name) {{ {PROFILE_FIELDS} }} }} {CARD_FRAGMENT}")
}

const FOLLOW_COUNTS_QUERY: &str = "query ($id: Int!) { followers: Page(perPage: 1) { pageInfo { total } \
followers(userId: $id) { id } } following: Page(perPage: 1) { pageInfo { total } following(userId: $id) { id } } }";

const IS_FOLLOWING_QUERY: &str = "query ($id: Int) { User(id: $id) { id isFollowing } }";
const TOGGLE_FOLLOW: &str = "mutation ($id: Int) { ToggleFollow(userId: $id) { id isFollowing } }";

fn users_query(edge: &str) -> String {
    format!(
        "query ($id: Int!, $page: Int, $perPage: Int) {{ Page(page: $page, perPage: $perPage) {{ pageInfo {{ currentPage hasNextPage }} \
{edge}(userId: $id) {{ {USER_FIELDS} }} }} }}"
    )
}

fn activity_fragments() -> String {
    format!(
        "__typename \
... on ListActivity {{ id status progress createdAt likeCount replyCount isLiked siteUrl user {{ {USER_FIELDS} }} media {{ ...mediaCard }} }} \
... on TextActivity {{ id text(asHtml: false) createdAt likeCount replyCount isLiked siteUrl user {{ {USER_FIELDS} }} }} \
... on MessageActivity {{ id message(asHtml: false) createdAt likeCount replyCount isLiked siteUrl messenger {{ {USER_FIELDS} }} }}"
    )
}

fn activities_query() -> String {
    format!(
        "query ($page: Int, $perPage: Int, $isFollowing: Boolean, $userId: Int, $mediaId: Int, $hasRepliesOrTypeText: Boolean) {{ \
Page(page: $page, perPage: $perPage) {{ pageInfo {{ hasNextPage }} activities(isFollowing: $isFollowing, userId: $userId, \
mediaId: $mediaId, hasRepliesOrTypeText: $hasRepliesOrTypeText, sort: [ID_DESC]) {{ {} }} }} }} {CARD_FRAGMENT}",
        activity_fragments()
    )
}

fn reply_fields() -> String {
    format!("id text(asHtml: false) createdAt likeCount isLiked user {{ {USER_FIELDS} }}")
}

fn replies_query() -> String {
    format!(
        "query ($id: Int, $page: Int) {{ Page(page: $page, perPage: 50) {{ pageInfo {{ hasNextPage }} \
activityReplies(activityId: $id) {{ {} }} }} }}",
        reply_fields()
    )
}

const ACTIVITY_LIKED_QUERY: &str = "query ($id: Int) { Activity(id: $id) { __typename \
... on ListActivity { id isLiked } ... on TextActivity { id isLiked } ... on MessageActivity { id isLiked } } }";
const TOGGLE_LIKE: &str = "mutation ($id: Int) { ToggleLikeV2(id: $id, type: ACTIVITY) { __typename } }";

fn save_reply_query() -> String {
    format!(
        "mutation ($activityId: Int, $text: String) {{ SaveActivityReply(activityId: $activityId, text: $text) {{ {} }} }}",
        reply_fields()
    )
}

fn save_text_query() -> String {
    format!(
        "mutation ($text: String) {{ SaveTextActivity(text: $text) {{ id text(asHtml: false) createdAt likeCount replyCount isLiked siteUrl \
user {{ {USER_FIELDS} }} }} }}"
    )
}

const DELETE_ACTIVITY: &str = "mutation ($id: Int) { DeleteActivity(id: $id) { deleted } }";

fn notifications_query() -> String {
    let user = format!("user {{ {USER_FIELDS} }}");
    let activity = format!("id type activityId context createdAt {user}");
    let thread = format!("id type context createdAt thread {{ id title siteUrl }} {user}");
    format!(
        "query ($page: Int, $perPage: Int, $reset: Boolean) {{ Page(page: $page, perPage: $perPage) {{ pageInfo {{ hasNextPage }} \
notifications(resetNotificationCount: $reset) {{ __typename \
... on AiringNotification {{ id type episode contexts createdAt media {{ ...mediaCard }} }} \
... on FollowingNotification {{ id type context createdAt {user} }} \
... on ActivityMessageNotification {{ {activity} }} \
... on ActivityMentionNotification {{ {activity} }} \
... on ActivityReplyNotification {{ {activity} }} \
... on ActivityReplySubscribedNotification {{ {activity} }} \
... on ActivityLikeNotification {{ {activity} }} \
... on ActivityReplyLikeNotification {{ {activity} }} \
... on ThreadCommentMentionNotification {{ {thread} }} \
... on ThreadCommentReplyNotification {{ {thread} }} \
... on ThreadCommentSubscribedNotification {{ {thread} }} \
... on ThreadCommentLikeNotification {{ {thread} }} \
... on ThreadLikeNotification {{ {thread} }} \
... on RelatedMediaAdditionNotification {{ id type context createdAt media {{ ...mediaCard }} }} \
... on MediaDataChangeNotification {{ id type context reason createdAt media {{ ...mediaCard }} }} \
... on MediaMergeNotification {{ id type context reason createdAt media {{ ...mediaCard }} }} \
... on MediaDeletionNotification {{ id type context reason deletedMediaTitle createdAt }} \
}} }} }} {CARD_FRAGMENT}"
    )
}

const UNREAD_QUERY: &str = "query { Viewer { id unreadNotificationCount } }";
const RESET_NOTIFICATIONS_QUERY: &str =
    "query { Page(page: 1, perPage: 1) { notifications(resetNotificationCount: true) { __typename } } }";

fn review_fields(with_body: bool) -> String {
    let body = if with_body { "body(asHtml: false)" } else { "" };
    format!("id summary {body} score rating ratingAmount createdAt siteUrl user {{ {USER_FIELDS} }}")
}

fn reviews_query() -> String {
    format!(
        "query ($id: Int, $idMal: Int, $page: Int, $perPage: Int) {{ Media(id: $id, idMal: $idMal, type: ANIME) {{ id averageScore \
reviews(page: $page, perPage: $perPage, sort: [RATING_DESC]) {{ pageInfo {{ hasNextPage }} nodes {{ {} }} }} }} }}",
        review_fields(false)
    )
}

fn review_query() -> String {
    format!(
        "query ($id: Int) {{ Review(id: $id) {{ {} }} }}",
        review_fields(true)
    )
}

const MEDIA_ID_QUERY: &str = "query ($idMal: Int) { Media(idMal: $idMal, type: ANIME) { id } }";

// ----- pure mappers -----

fn str_of(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn u32_of(v: &Value) -> u32 {
    v.as_u64().map_or(0, |n| n.min(u64::from(u32::MAX)) as u32)
}

/// `https://anilist.co/user/<name>` when AniList did not send `siteUrl`.
pub fn user_url(name: &str) -> String {
    format!("https://anilist.co/user/{name}")
}

/// `{ id name avatar siteUrl }` -> [`SocialUser`]; `None` without an id.
pub fn map_user(v: &Value) -> Option<SocialUser> {
    let id = v["id"].as_u64().filter(|id| *id > 0)?;
    let name = str_of(&v["name"]).unwrap_or_default();
    Some(SocialUser {
        id,
        avatar: str_of(&v["avatar"]["large"]).or_else(|| str_of(&v["avatar"]["medium"])),
        site_url: str_of(&v["siteUrl"]).unwrap_or_else(|| user_url(&name)),
        name,
    })
}

/// A `mediaCard` object -> [`SocialMedia`]; `None` when it is not an object with an id.
pub fn map_media(v: &Value) -> Option<SocialMedia> {
    if !v.is_object() || v["id"].as_u64().is_none_or(|id| id == 0) {
        return None;
    }
    Some(SocialMedia {
        media: map_anilist_media(v),
        format: str_of(&v["format"]),
    })
}

/// A `Viewer` / `User` object with the profile fields -> [`Profile`] (follow totals filled later).
pub fn map_profile(v: &Value) -> Option<Profile> {
    let user = map_user(v)?;
    let anime = &v["statistics"]["anime"];
    let manga = &v["statistics"]["manga"];
    let mut favourites: Vec<SocialMedia> = Vec::new();
    for edge in ["anime", "manga"] {
        for n in v["favourites"][edge]["nodes"].as_array().into_iter().flatten() {
            if let Some(m) = map_media(n) {
                favourites.push(m);
            }
        }
    }
    Some(Profile {
        user,
        banner: str_of(&v["bannerImage"]),
        about: v["about"].as_str().and_then(|a| clean_markdown(a)),
        stats: UserStats {
            anime_count: u32_of(&anime["count"]),
            episodes_watched: u32_of(&anime["episodesWatched"]),
            minutes_watched: anime["minutesWatched"].as_u64().unwrap_or(0),
            manga_count: u32_of(&manga["count"]),
            chapters_read: u32_of(&manga["chaptersRead"]),
            mean_score: anime["meanScore"].as_f64().filter(|s| *s > 0.0).map(|s| s as f32),
        },
        followers: 0,
        following: 0,
        is_following: v["isFollowing"].as_bool().unwrap_or(false),
        is_follower: v["isFollower"].as_bool().unwrap_or(false),
        favourites,
        created_at: v["createdAt"].as_i64().filter(|t| *t > 0),
    })
}

/// `{ followers: Page { pageInfo { total } }, following: ... }` -> `(followers, following)`.
pub fn map_follow_counts(data: &Value) -> (u32, u32) {
    (
        u32_of(&data["followers"]["pageInfo"]["total"]),
        u32_of(&data["following"]["pageInfo"]["total"]),
    )
}

/// `Page { pageInfo, followers | following }` -> [`UserPage`].
pub fn map_user_page(page: &Value, edge: &str, requested: u32) -> UserPage {
    UserPage {
        items: page[edge]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(map_user)
            .collect(),
        page: page["pageInfo"]["currentPage"]
            .as_u64()
            .map_or(requested.max(1), |p| p as u32),
        has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
    }
}

/// One `ActivityUnion` object -> [`Activity`]; unknown types and deleted users are skipped.
pub fn map_activity(v: &Value) -> Option<Activity> {
    let id = v["id"].as_u64()?;
    let (kind, user, text) = match v["__typename"].as_str()? {
        "ListActivity" => (ActivityKind::List, map_user(&v["user"])?, None),
        "TextActivity" => (
            ActivityKind::Text,
            map_user(&v["user"])?,
            v["text"].as_str().and_then(clean_markdown),
        ),
        "MessageActivity" => (
            ActivityKind::Message,
            map_user(&v["messenger"])?,
            v["message"].as_str().and_then(clean_markdown),
        ),
        _ => return None,
    };
    let progress = match &v["progress"] {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    };
    Some(Activity {
        id,
        kind,
        media: map_media(&v["media"]),
        status: str_of(&v["status"]),
        progress,
        text,
        created_at: v["createdAt"].as_i64().unwrap_or(0),
        likes: u32_of(&v["likeCount"]),
        liked: v["isLiked"].as_bool().unwrap_or(false),
        replies: u32_of(&v["replyCount"]),
        site_url: str_of(&v["siteUrl"]).unwrap_or_else(|| format!("https://anilist.co/activity/{id}")),
        user,
    })
}

/// `Page { pageInfo, activities }` -> activities, newest first as AniList sends them.
pub fn map_activities(page: &Value) -> Vec<Activity> {
    page["activities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(map_activity)
        .collect()
}

pub fn map_reply(v: &Value) -> Option<ActivityReply> {
    Some(ActivityReply {
        id: v["id"].as_u64()?,
        user: map_user(&v["user"])?,
        text: v["text"].as_str().and_then(clean_markdown).unwrap_or_default(),
        created_at: v["createdAt"].as_i64().unwrap_or(0),
        likes: u32_of(&v["likeCount"]),
        liked: v["isLiked"].as_bool().unwrap_or(false),
    })
}

fn media_title(m: &Option<SocialMedia>) -> String {
    m.as_ref().map(|m| m.media.title.clone()).unwrap_or_default()
}

/// One `NotificationUnion` object -> [`Notification`]. The fallback sentence follows anilist.co: airing =
/// `contexts[0] episode contexts[1] title contexts[2]`, media = `title context`, others = `user context`.
pub fn map_notification(v: &Value) -> Option<Notification> {
    let id = v["id"].as_u64()?;
    let kind = str_of(&v["type"]).or_else(|| v["__typename"].as_str().map(type_from_typename))?;
    let user = map_user(&v["user"]);
    let media = map_media(&v["media"]);
    let episode = v["episode"].as_u64().and_then(|e| u32::try_from(e).ok());
    let activity_id = v["activityId"].as_u64().filter(|a| *a > 0);
    let thread_title = str_of(&v["thread"]["title"]);
    let context = if kind == "AIRING" {
        let c: Vec<String> = v["contexts"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| c.as_str().unwrap_or_default().to_owned())
            .collect();
        let part = |i: usize| c.get(i).cloned().unwrap_or_default();
        let ep = episode.map(|e| e.to_string()).unwrap_or_default();
        format!("{}{}{}{}{}", part(0), ep, part(1), media_title(&media), part(2))
            .trim()
            .to_owned()
    } else {
        let ctx = v["context"].as_str().unwrap_or_default();
        let subject = if let Some(u) = &user {
            u.name.clone()
        } else if media.is_some() {
            media_title(&media)
        } else {
            v["deletedMediaTitle"].as_str().unwrap_or_default().to_owned()
        };
        let mut s = format!("{subject}{ctx}").trim().to_owned();
        if let Some(t) = &thread_title {
            s = format!("{s} {t}");
        }
        s
    };
    let url = if let Some(a) = activity_id {
        Some(format!("https://anilist.co/activity/{a}"))
    } else if let Some(t) = str_of(&v["thread"]["siteUrl"]) {
        Some(t)
    } else if let Some(m) = &media {
        m.media.media_ref.anilist_id.map(|id| {
            let kind = if m.media.kind == crate::model::MediaKind::Manga {
                "manga"
            } else {
                "anime"
            };
            format!("https://anilist.co/{kind}/{id}")
        })
    } else {
        user.as_ref().map(|u| u.site_url.clone())
    };
    Some(Notification {
        id,
        kind,
        user,
        media,
        episode,
        activity_id,
        thread_title,
        url,
        context,
        created_at: v["createdAt"].as_i64().unwrap_or(0),
    })
}

/// `ActivityLikeNotification` -> `ACTIVITY_LIKE` (only used when `type` is missing).
fn type_from_typename(t: &str) -> String {
    let base = t.strip_suffix("Notification").unwrap_or(t);
    let mut out = String::new();
    for (i, c) in base.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

pub fn map_notifications(page: &Value, requested: u32) -> NotificationPage {
    NotificationPage {
        items: page["notifications"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(map_notification)
            .collect(),
        page: requested.max(1),
        has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
    }
}

pub fn map_review(v: &Value) -> Option<Review> {
    let id = v["id"].as_u64()?;
    Some(Review {
        id,
        user: map_user(&v["user"])?,
        summary: v["summary"].as_str().and_then(clean_markdown).unwrap_or_default(),
        body: v["body"].as_str().and_then(clean_markdown),
        score: v["score"].as_u64().filter(|s| *s > 0).map(|s| s.min(100) as u32),
        rating: v["rating"]
            .as_i64()
            .map_or(0, |r| r.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32),
        rating_amount: u32_of(&v["ratingAmount"]),
        created_at: v["createdAt"].as_i64().unwrap_or(0),
        site_url: str_of(&v["siteUrl"]).unwrap_or_else(|| format!("https://anilist.co/review/{id}")),
    })
}

/// `Media { id averageScore reviews { pageInfo nodes } }` -> [`ReviewPage`]; a missing media is an empty page.
pub fn map_review_page(media: &Value, requested: u32) -> ReviewPage {
    ReviewPage {
        items: media["reviews"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(map_review)
            .collect(),
        page: requested.max(1),
        has_next: media["reviews"]["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false),
        media_id: media["id"].as_u64().and_then(|i| u32::try_from(i).ok()),
        average_score: media["averageScore"]
            .as_u64()
            .filter(|s| *s > 0)
            .map(|s| s.min(100) as u32),
    }
}

/// Variables of [`activities_query`] for a feed.
pub fn activity_vars(feed: &ActivityFeed, page: u32) -> Value {
    let mut v = serde_json::Map::new();
    v.insert("page".into(), page.max(1).into());
    v.insert("perPage".into(), ACTIVITIES_PER_PAGE.into());
    match feed {
        ActivityFeed::Following => {
            v.insert("isFollowing".into(), true.into());
        }
        ActivityFeed::Global => {
            v.insert("hasRepliesOrTypeText".into(), true.into());
        }
        ActivityFeed::User(id) => {
            v.insert("userId".into(), (*id).into());
        }
        ActivityFeed::Media { media_id, user_id } => {
            v.insert("mediaId".into(), (*media_id).into());
            if let Some(u) = user_id {
                v.insert("userId".into(), (*u).into());
            }
        }
    }
    Value::Object(v)
}

/// Trimmed text within AniList's limit; `None` when empty.
pub fn post_text(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.chars().take(MAX_TEXT_CHARS).collect())
}

/// AniList markdown -> plain text: drops `img<w>(...)`, `webm(...)`, `youtube(...)`, hides `~!spoilers!~` behind
/// `[…]`, keeps link text, removes emphasis / centering / heading markers, then HTML (`<br>` = newline, tags
/// stripped, blank lines collapsed) through `clean_synopsis`. `None` when nothing is left.
pub fn clean_markdown(text: &str) -> Option<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let at_word_start = |i: usize| i == 0 || !chars[i - 1].is_alphanumeric();
    while i < chars.len() {
        // Embeds: img220(url), img(url), img50%(url), webm(url), youtube(id).
        if at_word_start(i) {
            let mut matched = None;
            for word in ["img", "webm", "youtube"] {
                let w: Vec<char> = word.chars().collect();
                if chars[i..].starts_with(&w) {
                    let mut j = i + w.len();
                    while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '%') {
                        j += 1;
                    }
                    if j < chars.len()
                        && chars[j] == '('
                        && let Some(rel) = chars[j..].iter().position(|c| *c == ')')
                    {
                        matched = Some(j + rel + 1);
                        break;
                    }
                }
            }
            if let Some(end) = matched {
                i = end;
                continue;
            }
        }
        // Spoilers ~! ... !~
        if chars[i..].starts_with(&['~', '!']) {
            let rest = &chars[i + 2..];
            if let Some(rel) = rest.windows(2).position(|w| w == ['!', '~']) {
                out.push_str("[…]");
                i += 2 + rel + 2;
                continue;
            }
        }
        // Centering ~~~ and strike ~~
        if chars[i..].starts_with(&['~', '~']) {
            i += if chars[i..].starts_with(&['~', '~', '~']) {
                3
            } else {
                2
            };
            continue;
        }
        // Bold / italic markers __ and **
        if chars[i..].starts_with(&['_', '_']) || chars[i..].starts_with(&['*', '*']) {
            i += 2;
            continue;
        }
        // Links [text](url) -> text
        if chars[i] == '['
            && let Some(close) = chars[i..].iter().position(|c| *c == ']')
            && chars.get(i + close + 1) == Some(&'(')
            && let Some(paren) = chars[i + close + 1..].iter().position(|c| *c == ')')
        {
            out.extend(&chars[i + 1..i + close]);
            i += close + 1 + paren + 1;
            continue;
        }
        // Headings at the start of a line
        if chars[i] == '#' && (i == 0 || chars[i - 1] == '\n') {
            while i < chars.len() && chars[i] == '#' {
                i += 1;
            }
            while i < chars.len() && chars[i] == ' ' {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    // `\n` from markdown line breaks survive `clean_synopsis` (it only rewrites <br>).
    clean_synopsis(Some(out.trim())).filter(|s| !s.trim().is_empty())
}

// ----- client -----

impl AniList {
    /// Follower / following totals of a user.
    async fn follow_counts(&self, token: Option<&str>, user_id: u64) -> Result<(u32, u32), ListError> {
        let body = self
            .post_auth(token, FOLLOW_COUNTS_QUERY, json!({ "id": user_id }))
            .await?;
        Ok(map_follow_counts(&body["data"]))
    }

    async fn with_counts(&self, token: Option<&str>, mut p: Profile) -> Result<Profile, ListError> {
        // The totals are a second, cheap request; a failure there must not hide the profile.
        if let Ok((followers, following)) = self.follow_counts(token, p.user.id).await {
            p.followers = followers;
            p.following = following;
        }
        Ok(p)
    }

    /// The token owner's profile.
    pub async fn viewer_profile(&self, token: &str) -> Result<Profile, ListError> {
        let body = self
            .post_auth(Some(token), &viewer_profile_query(), json!({}))
            .await?;
        let p = map_profile(&body["data"]["Viewer"]).ok_or_else(|| ListError::Parse("Viewer".into()))?;
        self.with_counts(Some(token), p).await
    }

    /// A public profile by user name; `Ok(None)` when AniList has no such user.
    pub async fn user_profile(&self, token: Option<&str>, name: &str) -> Result<Option<Profile>, ListError> {
        let body = match self
            .post_auth(token, &user_profile_query(), json!({ "name": name }))
            .await
        {
            Ok(b) => b,
            Err(ListError::Http(404)) => return Ok(None),
            Err(ListError::Parse(m)) if m.to_ascii_lowercase().contains("not found") => return Ok(None),
            Err(e) => return Err(e),
        };
        match map_profile(&body["data"]["User"]) {
            Some(p) => Ok(Some(self.with_counts(token, p).await?)),
            None => Ok(None),
        }
    }

    /// Follow (`true`) or unfollow a user; `ToggleFollow` only runs when the state differs. Returns the new state.
    pub async fn set_follow(&self, token: &str, user_id: u64, follow: bool) -> Result<bool, ListError> {
        let body = self
            .post_auth(Some(token), IS_FOLLOWING_QUERY, json!({ "id": user_id }))
            .await?;
        if body["data"]["User"]["isFollowing"].as_bool().unwrap_or(false) == follow {
            return Ok(follow);
        }
        let body = self
            .post_auth(Some(token), TOGGLE_FOLLOW, json!({ "id": user_id }))
            .await?;
        Ok(body["data"]["ToggleFollow"]["isFollowing"]
            .as_bool()
            .unwrap_or(follow))
    }

    async fn users(
        &self,
        token: Option<&str>,
        edge: &str,
        user_id: u64,
        page: u32,
    ) -> Result<UserPage, ListError> {
        let vars = json!({ "id": user_id, "page": page.max(1), "perPage": USERS_PER_PAGE });
        let body = self.post_auth(token, &users_query(edge), vars).await?;
        Ok(map_user_page(&body["data"]["Page"], edge, page))
    }

    pub async fn followers(
        &self,
        token: Option<&str>,
        user_id: u64,
        page: u32,
    ) -> Result<UserPage, ListError> {
        self.users(token, "followers", user_id, page).await
    }

    pub async fn following(
        &self,
        token: Option<&str>,
        user_id: u64,
        page: u32,
    ) -> Result<UserPage, ListError> {
        self.users(token, "following", user_id, page).await
    }

    /// A page of activities, newest first. `Following` needs the token.
    pub async fn activities(
        &self,
        token: Option<&str>,
        feed: &ActivityFeed,
        page: u32,
    ) -> Result<Vec<Activity>, ListError> {
        let body = self
            .post_auth(token, &activities_query(), activity_vars(feed, page))
            .await?;
        Ok(map_activities(&body["data"]["Page"]))
    }

    /// The replies of an activity, oldest first (the order AniList threads them in).
    pub async fn activity_replies(
        &self,
        token: Option<&str>,
        activity_id: u64,
    ) -> Result<Vec<ActivityReply>, ListError> {
        let body = self
            .post_auth(token, &replies_query(), json!({ "id": activity_id, "page": 1 }))
            .await?;
        Ok(body["data"]["Page"]["activityReplies"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(map_reply)
            .collect())
    }

    /// Like / unlike an activity; `ToggleLikeV2` only runs when the state differs.
    pub async fn set_activity_like(
        &self,
        token: &str,
        activity_id: u64,
        like: bool,
    ) -> Result<(), ListError> {
        let body = self
            .post_auth(Some(token), ACTIVITY_LIKED_QUERY, json!({ "id": activity_id }))
            .await?;
        if body["data"]["Activity"]["isLiked"].as_bool().unwrap_or(false) == like {
            return Ok(());
        }
        self.post_auth(Some(token), TOGGLE_LIKE, json!({ "id": activity_id }))
            .await
            .map(|_| ())
    }

    pub async fn save_reply(
        &self,
        token: &str,
        activity_id: u64,
        text: &str,
    ) -> Result<ActivityReply, ListError> {
        let text = post_text(text).ok_or_else(|| ListError::Parse("empty reply".into()))?;
        let body = self
            .post_auth(
                Some(token),
                &save_reply_query(),
                json!({ "activityId": activity_id, "text": text }),
            )
            .await?;
        map_reply(&body["data"]["SaveActivityReply"])
            .ok_or_else(|| ListError::Parse("SaveActivityReply".into()))
    }

    pub async fn save_text_activity(&self, token: &str, text: &str) -> Result<Activity, ListError> {
        let text = post_text(text).ok_or_else(|| ListError::Parse("empty status".into()))?;
        let body = self
            .post_auth(Some(token), &save_text_query(), json!({ "text": text }))
            .await?;
        let mut a = body["data"]["SaveTextActivity"].clone();
        if let Some(o) = a.as_object_mut() {
            o.insert("__typename".into(), "TextActivity".into());
        }
        map_activity(&a).ok_or_else(|| ListError::Parse("SaveTextActivity".into()))
    }

    pub async fn delete_activity(&self, token: &str, activity_id: u64) -> Result<(), ListError> {
        match self
            .post_auth(Some(token), DELETE_ACTIVITY, json!({ "id": activity_id }))
            .await
        {
            Ok(_) => Ok(()),
            // Already gone: the goal is reached.
            Err(ListError::Http(404)) => Ok(()),
            Err(ListError::Parse(m)) if m.to_ascii_lowercase().contains("not found") => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// A page of the viewer's notifications without resetting the unread count.
    pub async fn notifications(&self, token: &str, page: u32) -> Result<NotificationPage, ListError> {
        let vars = json!({ "page": page.max(1), "perPage": NOTIFICATIONS_PER_PAGE, "reset": false });
        let body = self.post_auth(Some(token), &notifications_query(), vars).await?;
        Ok(map_notifications(&body["data"]["Page"], page))
    }

    pub async fn unread_count(&self, token: &str) -> Result<u32, ListError> {
        let body = self.post_auth(Some(token), UNREAD_QUERY, json!({})).await?;
        Ok(u32_of(&body["data"]["Viewer"]["unreadNotificationCount"]))
    }

    /// "Mark all read": AniList resets the count when notifications are read with `resetNotificationCount: true`.
    pub async fn reset_notifications(&self, token: &str) -> Result<(), ListError> {
        self.post_auth(Some(token), RESET_NOTIFICATIONS_QUERY, json!({}))
            .await
            .map(|_| ())
    }

    /// Reviews of an anime, best rated first, with its average score.
    pub async fn reviews(
        &self,
        token: Option<&str>,
        media: MediaKey,
        page: u32,
    ) -> Result<ReviewPage, ListError> {
        let (id, id_mal) = match media {
            MediaKey::AniList(id) => (Some(id), None),
            MediaKey::Mal(id) => (None, Some(id)),
        };
        let vars = json!({ "id": id, "idMal": id_mal, "page": page.max(1), "perPage": REVIEWS_PER_PAGE });
        match self.post_auth(token, &reviews_query(), vars).await {
            Ok(body) => Ok(map_review_page(&body["data"]["Media"], page)),
            Err(ListError::Http(404)) => Ok(ReviewPage {
                page: page.max(1),
                ..ReviewPage::default()
            }),
            Err(ListError::Parse(m)) if m.to_ascii_lowercase().contains("not found") => Ok(ReviewPage {
                page: page.max(1),
                ..ReviewPage::default()
            }),
            Err(e) => Err(e),
        }
    }

    /// One review with its full body.
    pub async fn review(&self, token: Option<&str>, review_id: u64) -> Result<Review, ListError> {
        let body = self
            .post_auth(token, &review_query(), json!({ "id": review_id }))
            .await?;
        map_review(&body["data"]["Review"]).ok_or_else(|| ListError::Parse("Review".into()))
    }

    /// AniList id of a title named by its MAL id (`None` when AniList does not know it).
    pub async fn anilist_id_of(
        &self,
        token: Option<&str>,
        media: MediaKey,
    ) -> Result<Option<u32>, ListError> {
        match media {
            MediaKey::AniList(id) => Ok(Some(id)),
            MediaKey::Mal(mal) => match self
                .post_auth(token, MEDIA_ID_QUERY, json!({ "idMal": mal }))
                .await
            {
                Ok(body) => Ok(body["data"]["Media"]["id"]
                    .as_u64()
                    .and_then(|i| u32::try_from(i).ok())),
                Err(ListError::Http(404)) => Ok(None),
                Err(ListError::Parse(m)) if m.to_ascii_lowercase().contains("not found") => Ok(None),
                Err(e) => Err(e),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaKind;

    fn user(id: u64, name: &str) -> Value {
        json!({ "id": id, "name": name, "avatar": { "large": format!("https://img/{name}.png") }, "siteUrl": format!("https://anilist.co/user/{name}") })
    }

    fn card(id: u64, kind: &str, format: &str) -> Value {
        json!({ "id": id, "type": kind, "format": format, "title": { "romaji": format!("T{id}") }, "coverImage": {} })
    }

    #[test]
    fn users_need_an_id_and_get_a_url() {
        let u = map_user(&json!({ "id": 7, "name": "Ana" })).unwrap();
        assert_eq!(u.site_url, "https://anilist.co/user/Ana");
        assert_eq!(u.avatar, None);
        assert!(map_user(&json!({ "name": "x" })).is_none());
        assert!(map_user(&Value::Null).is_none());
        assert_eq!(
            map_user(&user(3, "Bo")).unwrap().avatar.as_deref(),
            Some("https://img/Bo.png")
        );
    }

    #[test]
    fn profile_maps_stats_favourites_and_flags() {
        let mut v = user(5, "Kai");
        let o = v.as_object_mut().unwrap();
        o.insert("about".into(), json!("Hi __there__ img220(https://x/y.png)"));
        o.insert("bannerImage".into(), json!("https://banner"));
        o.insert("isFollowing".into(), json!(true));
        o.insert("createdAt".into(), json!(1_600_000_000));
        o.insert(
            "statistics".into(),
            json!({ "anime": { "count": 120, "episodesWatched": 1500, "minutesWatched": 36000, "meanScore": 74.5 }, "manga": { "count": 3, "chaptersRead": 90 } }),
        );
        o.insert(
            "favourites".into(),
            json!({ "anime": { "nodes": [card(1, "ANIME", "TV")] }, "manga": { "nodes": [card(2, "MANGA", "NOVEL"), { "id": 0 }] } }),
        );
        let p = map_profile(&v).unwrap();
        assert_eq!(p.about.as_deref(), Some("Hi there"));
        assert_eq!(p.stats.anime_count, 120);
        assert_eq!(p.stats.minutes_watched, 36000);
        assert_eq!(p.stats.chapters_read, 90);
        assert_eq!(p.stats.mean_score, Some(74.5));
        assert!(p.is_following && !p.is_follower);
        assert_eq!(p.favourites.len(), 2);
        assert_eq!(p.favourites[1].media.kind, MediaKind::Manga);
        assert_eq!(p.favourites[1].format.as_deref(), Some("NOVEL"));
        assert_eq!(p.created_at, Some(1_600_000_000));
    }

    #[test]
    fn follow_counts_and_user_pages() {
        let data = json!({ "followers": { "pageInfo": { "total": 12 } }, "following": { "pageInfo": { "total": 3 } } });
        assert_eq!(map_follow_counts(&data), (12, 3));
        let page = json!({ "pageInfo": { "currentPage": 2, "hasNextPage": true }, "followers": [user(1, "a"), { "id": null }, user(2, "b")] });
        let p = map_user_page(&page, "followers", 2);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.page, 2);
        assert!(p.has_next);
        assert_eq!(map_user_page(&Value::Null, "following", 0).page, 1);
    }

    #[test]
    fn activities_of_the_three_kinds() {
        let page = json!({ "activities": [
            { "__typename": "ListActivity", "id": 10, "status": "watched episode", "progress": "3 - 4", "createdAt": 100,
              "likeCount": 2, "replyCount": 1, "isLiked": true, "siteUrl": "https://anilist.co/activity/10", "user": user(1, "a"), "media": card(21, "ANIME", "TV") },
            { "__typename": "TextActivity", "id": 11, "text": "Hello ~!secret!~ world", "createdAt": 90, "user": user(2, "b") },
            { "__typename": "MessageActivity", "id": 12, "message": "msg", "createdAt": 80, "messenger": user(3, "c") },
            { "__typename": "UnknownActivity", "id": 13 },
            { "__typename": "TextActivity", "id": 14, "text": "deleted user", "user": null }
        ] });
        let list = map_activities(&page);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].kind, ActivityKind::List);
        assert_eq!(list[0].progress.as_deref(), Some("3 - 4"));
        assert_eq!(
            list[0].media.as_ref().unwrap().media.media_ref.anilist_id,
            Some(21)
        );
        assert!(list[0].liked);
        assert_eq!(list[0].replies, 1);
        assert_eq!(list[1].text.as_deref(), Some("Hello […] world"));
        assert_eq!(list[1].site_url, "https://anilist.co/activity/11");
        assert_eq!(list[2].kind, ActivityKind::Message);
        assert_eq!(list[2].user.name, "c");
    }

    #[test]
    fn numeric_progress_is_kept() {
        let a = map_activity(
            &json!({ "__typename": "ListActivity", "id": 1, "progress": 12, "user": user(1, "a") }),
        )
        .unwrap();
        assert_eq!(a.progress.as_deref(), Some("12"));
    }

    #[test]
    fn replies_map() {
        let r = map_reply(&json!({ "id": 4, "text": "**ok**", "createdAt": 5, "likeCount": 1, "isLiked": false, "user": user(9, "z") })).unwrap();
        assert_eq!(r.text, "ok");
        assert_eq!(r.user.id, 9);
        assert!(map_reply(&json!({ "id": 4 })).is_none());
    }

    #[test]
    fn notification_sentences_and_links() {
        let airing = map_notification(
            &json!({ "__typename": "AiringNotification", "id": 1, "type": "AIRING", "episode": 5,
            "contexts": ["Episode ", " of ", " aired."], "createdAt": 10, "media": card(21, "ANIME", "TV") }),
        )
        .unwrap();
        assert_eq!(airing.context, "Episode 5 of T21 aired.");
        assert_eq!(airing.episode, Some(5));
        assert_eq!(airing.url.as_deref(), Some("https://anilist.co/anime/21"));

        let follow = map_notification(&json!({ "id": 2, "type": "FOLLOWING", "context": " started following you.", "user": user(3, "Mia") })).unwrap();
        assert_eq!(follow.context, "Mia started following you.");
        assert_eq!(follow.url.as_deref(), Some("https://anilist.co/user/Mia"));

        let like = map_notification(&json!({ "__typename": "ActivityLikeNotification", "id": 3, "activityId": 77, "context": " liked your activity.", "user": user(4, "Leo") })).unwrap();
        assert_eq!(like.kind, "ACTIVITY_LIKE");
        assert_eq!(like.url.as_deref(), Some("https://anilist.co/activity/77"));

        let thread = map_notification(&json!({ "id": 5, "type": "THREAD_LIKE", "context": " liked your forum thread", "user": user(4, "Leo"),
            "thread": { "title": "Best OP", "siteUrl": "https://anilist.co/forum/thread/1" } }))
        .unwrap();
        assert_eq!(thread.context, "Leo liked your forum thread Best OP");
        assert_eq!(thread.url.as_deref(), Some("https://anilist.co/forum/thread/1"));

        let deleted = map_notification(&json!({ "id": 6, "type": "MEDIA_DELETION", "context": " was deleted from the site", "deletedMediaTitle": "Old" })).unwrap();
        assert_eq!(deleted.context, "Old was deleted from the site");
        assert_eq!(deleted.url, None);

        assert!(map_notification(&json!({ "type": "AIRING" })).is_none());
    }

    #[test]
    fn typename_to_type() {
        assert_eq!(
            type_from_typename("ActivityReplyLikeNotification"),
            "ACTIVITY_REPLY_LIKE"
        );
        assert_eq!(type_from_typename("FollowingNotification"), "FOLLOWING");
    }

    #[test]
    fn notification_page() {
        let page = json!({ "pageInfo": { "hasNextPage": true }, "notifications": [{ "id": 1, "type": "FOLLOWING", "context": " x", "user": user(1, "a") }, {}] });
        let p = map_notifications(&page, 0);
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.page, 1);
        assert!(p.has_next);
    }

    #[test]
    fn reviews_map_with_average() {
        let media = json!({ "id": 21, "averageScore": 88, "reviews": { "pageInfo": { "hasNextPage": false }, "nodes": [
            { "id": 9, "summary": "Great", "score": 95, "rating": 40, "ratingAmount": 50, "createdAt": 1, "user": user(1, "a") },
            { "id": 10, "summary": "No user", "user": null }
        ] } });
        let p = map_review_page(&media, 1);
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.average_score, Some(88));
        assert_eq!(p.media_id, Some(21));
        assert_eq!(p.items[0].score, Some(95));
        assert_eq!(p.items[0].site_url, "https://anilist.co/review/9");
        let empty = map_review_page(&Value::Null, 3);
        assert!(empty.items.is_empty() && empty.average_score.is_none() && empty.page == 3);
        let r = map_review(
            &json!({ "id": 1, "summary": "s", "body": "Line<br>two", "score": 0, "user": user(1, "a") }),
        )
        .unwrap();
        assert_eq!(r.body.as_deref(), Some("Line\ntwo"));
        assert_eq!(r.score, None);
    }

    #[test]
    fn feed_variables() {
        assert_eq!(activity_vars(&ActivityFeed::Following, 0)["isFollowing"], true);
        assert_eq!(activity_vars(&ActivityFeed::Following, 0)["page"], 1);
        assert_eq!(
            activity_vars(&ActivityFeed::Global, 2)["hasRepliesOrTypeText"],
            true
        );
        assert_eq!(activity_vars(&ActivityFeed::User(5), 1)["userId"], 5);
        let m = activity_vars(
            &ActivityFeed::Media {
                media_id: 21,
                user_id: Some(4),
            },
            1,
        );
        assert_eq!(m["mediaId"], 21);
        assert_eq!(m["userId"], 4);
        assert!(
            activity_vars(
                &ActivityFeed::Media {
                    media_id: 21,
                    user_id: None
                },
                1
            )
            .get("userId")
            .is_none()
        );
    }

    #[test]
    fn post_text_trims_and_caps() {
        assert_eq!(post_text("   "), None);
        assert_eq!(post_text(" hi ").as_deref(), Some("hi"));
        assert_eq!(
            post_text(&"x".repeat(3000)).unwrap().chars().count(),
            MAX_TEXT_CHARS
        );
    }

    #[test]
    fn markdown_is_flattened() {
        assert_eq!(clean_markdown("~~~img220(https://a/b.gif)~~~").as_deref(), None);
        assert_eq!(
            clean_markdown("# Title\n__bold__ and **strong** [link](https://x)").as_deref(),
            Some("Title\nbold and strong link")
        );
        assert_eq!(
            clean_markdown("see youtube(abc) webm(https://v) now").as_deref(),
            Some("see   now")
        );
        assert_eq!(clean_markdown("a ~!b!~ c").as_deref(), Some("a […] c"));
        assert_eq!(
            clean_markdown("unclosed ~!spoiler").as_deref(),
            Some("unclosed ~!spoiler")
        );
        assert_eq!(
            clean_markdown("imgur is a word").as_deref(),
            Some("imgur is a word")
        );
        assert_eq!(clean_markdown("<b>x</b><br>y").as_deref(), Some("x\ny"));
        assert_eq!(
            clean_markdown("[not a link] ok").as_deref(),
            Some("[not a link] ok")
        );
    }
}
