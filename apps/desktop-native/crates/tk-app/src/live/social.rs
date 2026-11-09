//! AniList social (wave B, worker **n**; plan 14 section 0.1 #10; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `social.*`, `details.reviews`). All of it is AniList GraphQL
//! (`tk_core::metadata::anilist::social`: `Viewer`, `User(name)`, `Page.followers / following`, `Page.activities`,
//! `ToggleFollow`, `ToggleLikeV2`, `SaveActivityReply`, `SaveTextActivity`, `DeleteActivity`,
//! `Page.notifications(resetNotificationCount)`, `Viewer.unreadNotificationCount`, `Media.reviews`, `Review(id)`)
//! with the token worker g stores in `b.secrets` (`tk_core::secrets::keys::ANILIST_TOKEN`, JSON `lists::Token`).
//!
//! Without a token the calls that need an account answer `ErrorKind::Auth` (`not_connected`; `expired` when AniList
//! rejects it) and the UI shows "connect AniList". Public reads (another user's profile, followers lists, the global
//! feed, reviews) also work without one. Short-lived cache in `social_cache` (`tk-core/src/store/social.rs`) keyed by
//! account so the rail badge, Details and the profile pages do not hammer AniList (90 requests / minute); writes
//! drop the keys they make stale. Activity and notifications are never cached (likes and read state move).

use std::sync::OnceLock;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use tk_core::ListError;
use tk_core::MediaKind;
use tk_core::lists::{self, Service, Token};
use tk_core::metadata::anilist::AniList;
use tk_core::metadata::anilist::social::{
    Activity, ActivityFeed, ActivityKind, ActivityReply, MediaKey, NOTIFICATIONS_PER_PAGE, Notification,
    Profile, Review, ReviewPage, SocialMedia, SocialUser, UserPage,
};
use tk_ui::backend::{
    ActivityFeedVm, ActivityKindVm, ActivityReplyVm, ActivityVm, MediaVm, NotificationPageVm, NotificationVm,
    ProfileStatsVm, ProfileVm, ReviewVm, UiResult, UserPageVm, UserVm,
};
use tk_ui::model::social::{MANGA_PREFIX, NOVEL_PREFIX, notification_text, unread_part};
use tk_ui::model::{ErrorKind, UiError};

use crate::backend_common::{IdKind, parse_media_id};
use crate::live::LiveBackend;

const PROFILE_TTL: i64 = 5 * 60;
const FOLLOWS_TTL: i64 = 5 * 60;
const UNREAD_TTL: i64 = 60;
const REVIEWS_TTL: i64 = 30 * 60;
const REVIEW_TTL: i64 = 60 * 60;
const MEDIA_ID_TTL: i64 = 7 * 24 * 60 * 60;

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

fn anilist() -> AniList {
    AniList::new(client().clone())
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

// ----- errors -----

/// `ListError` -> `UiError` (same detail codes as Settings > Accounts: `not_connected`, `expired`).
fn social_err(e: ListError) -> UiError {
    match e {
        ListError::NotConnected => UiError::new(ErrorKind::Auth, "not_connected"),
        ListError::Unauthorized => UiError::new(ErrorKind::Auth, "expired"),
        ListError::OAuth(code) => UiError::new(ErrorKind::Auth, code),
        ListError::RateLimited => UiError::new(ErrorKind::RateLimited, "rate limited"),
        ListError::Network(m) => UiError::new(ErrorKind::Offline, m),
        other => UiError::new(ErrorKind::Other, other.to_string()),
    }
}

fn not_found(what: &str) -> UiError {
    UiError::new(ErrorKind::Other, format!("not_found: {what}"))
}

// ----- token -----

/// The stored AniList token, if any and not expired. A token AniList rejects later surfaces as `expired`.
fn stored_token(b: &LiveBackend) -> Result<Token, ListError> {
    let json = b
        .secrets
        .get(Service::AniList.secret_key())?
        .ok_or(ListError::NotConnected)?;
    let t = serde_json::from_str::<Token>(&json)
        .ok()
        .filter(|t| t.service == Service::AniList && !t.access_token.is_empty())
        .ok_or(ListError::NotConnected)?;
    if lists::is_expired(&t, now()) {
        return Err(ListError::Unauthorized);
    }
    Ok(t)
}

fn token(b: &LiveBackend) -> UiResult<Token> {
    stored_token(b).map_err(social_err)
}

/// For public reads: the token when there is a usable one (likes / follow state need it), else none.
fn optional_token(b: &LiveBackend) -> Option<Token> {
    stored_token(b).ok()
}

/// Cache namespace of the account (`anon` without one), so switching accounts never shows the other's data.
fn account_tag(t: Option<&Token>) -> String {
    match t {
        Some(t) => t
            .user_id
            .map(|i| i.to_string())
            .or_else(|| t.username.clone())
            .unwrap_or_else(|| "me".into()),
        None => "anon".into(),
    }
}

fn cache_key(t: Option<&Token>, key: &str) -> String {
    format!("{}|{key}", account_tag(t))
}

/// The viewer's AniList id: from the token (g stores it at connect time) or `Viewer` once, cached.
async fn viewer_id(b: &LiveBackend, t: &Token) -> UiResult<u64> {
    if let Some(id) = t.user_id {
        return Ok(id);
    }
    let key = cache_key(Some(t), "viewer_id");
    if let Some(id) = cached::<u64>(b, &key) {
        return Ok(id);
    }
    let v = anilist().viewer(&t.access_token).await.map_err(social_err)?;
    put(b, &key, &v.id, PROFILE_TTL * 12);
    Ok(v.id)
}

// ----- cache -----

fn cached<T: DeserializeOwned>(b: &LiveBackend, key: &str) -> Option<T> {
    match b.store.social_get(key, now()) {
        Ok(Some(json)) => serde_json::from_str(&json).ok(),
        Ok(None) => None,
        Err(e) => {
            tracing::debug!(error = %e, key, "social cache read failed");
            None
        }
    }
}

fn put<T: Serialize>(b: &LiveBackend, key: &str, value: &T, ttl: i64) {
    let Ok(json) = serde_json::to_string(value) else {
        return;
    };
    let at = now();
    if let Err(e) = b.store.social_put(key, &json, at + ttl) {
        tracing::debug!(error = %e, key, "social cache write failed");
    }
    // Opportunistic clean-up of what expired meanwhile.
    let _ = b.store.social_prune(at);
}

fn forget(b: &LiveBackend, t: Option<&Token>, prefix: &str) {
    if let Err(e) = b.store.social_forget(&cache_key(t, prefix)) {
        tracing::debug!(error = %e, prefix, "social cache invalidation failed");
    }
}

// ----- core -> view-model -----

fn user_vm(u: &SocialUser) -> UserVm {
    UserVm {
        id: u.id,
        name: u.name.clone(),
        avatar: u.avatar.clone(),
        url: u.site_url.clone(),
    }
}

/// Card of a title; manga / novels get the `manga:` / `novel:` prefix so the social pages open the right page.
fn media_vm(m: &SocialMedia) -> MediaVm {
    let mut vm = super::to_vm(&m.media);
    if m.media.kind == MediaKind::Manga
        && let Some(id) = m.media.media_ref.anilist_id
    {
        let prefix = if m.format.as_deref() == Some("NOVEL") {
            NOVEL_PREFIX
        } else {
            MANGA_PREFIX
        };
        vm.id = format!("{prefix}{id}");
    }
    vm
}

fn profile_vm(p: &Profile, is_me: bool) -> ProfileVm {
    ProfileVm {
        user: user_vm(&p.user),
        banner: p.banner.clone(),
        about: p.about.clone(),
        stats: ProfileStatsVm {
            anime_count: p.stats.anime_count,
            episodes_watched: p.stats.episodes_watched,
            minutes_watched: p.stats.minutes_watched,
            manga_count: p.stats.manga_count,
            chapters_read: p.stats.chapters_read,
            mean_score: p.stats.mean_score,
        },
        followers: p.followers,
        following: p.following,
        is_following: !is_me && p.is_following,
        is_follower: !is_me && p.is_follower,
        is_me,
        favorites: p.favourites.iter().map(media_vm).collect(),
        created_unix: p.created_at,
    }
}

fn activity_vm(a: &Activity) -> ActivityVm {
    ActivityVm {
        id: a.id,
        kind: match a.kind {
            ActivityKind::List => ActivityKindVm::List,
            ActivityKind::Text => ActivityKindVm::Text,
            ActivityKind::Message => ActivityKindVm::Message,
        },
        user: user_vm(&a.user),
        media: a.media.as_ref().map(media_vm),
        status: a.status.clone(),
        progress: a.progress.clone(),
        text: a.text.clone(),
        created_unix: a.created_at,
        likes: a.likes,
        liked: a.liked,
        replies: a.replies,
        url: a.site_url.clone(),
    }
}

fn reply_vm(r: &ActivityReply) -> ActivityReplyVm {
    ActivityReplyVm {
        id: r.id,
        user: user_vm(&r.user),
        text: r.text.clone(),
        created_unix: r.created_at,
        likes: r.likes,
        liked: r.liked,
    }
}

fn notification_vm(n: &Notification) -> NotificationVm {
    let media = n.media.as_ref().map(media_vm);
    NotificationVm {
        id: n.id,
        kind: n.kind.clone(),
        text: notification_text(
            &n.kind,
            n.user.as_ref().map(|u| u.name.as_str()),
            media.as_ref().map(|m| m.title.as_str()),
            n.episode,
            n.thread_title.as_deref(),
            &n.context,
        ),
        media,
        user: n.user.as_ref().map(user_vm),
        created_unix: n.created_at,
        url: n.url.clone(),
    }
}

fn review_vm(r: &Review) -> ReviewVm {
    ReviewVm {
        id: r.id,
        user: user_vm(&r.user),
        summary: r.summary.clone(),
        body: r.body.clone(),
        score: r.score,
        rating: r.rating,
        rating_amount: r.rating_amount,
        created_unix: r.created_at,
        url: r.site_url.clone(),
    }
}

fn user_page_vm(p: &UserPage) -> UserPageVm {
    UserPageVm {
        items: p.items.iter().map(user_vm).collect(),
        page: p.page,
        has_next: p.has_next,
    }
}

/// A UI media id as AniList knows it (`None` for Kitsu ids and provider titles: AniList has no data for them).
fn media_key(media_id: &str) -> Option<MediaKey> {
    match parse_media_id(media_id)? {
        (IdKind::AniList, id) => Some(MediaKey::AniList(id)),
        (IdKind::Mal, id) => Some(MediaKey::Mal(id)),
        (IdKind::Kitsu, _) => None,
    }
}

async fn anilist_media_id(b: &LiveBackend, t: Option<&Token>, key: MediaKey) -> UiResult<Option<u32>> {
    let MediaKey::Mal(mal) = key else {
        return anilist().anilist_id_of(None, key).await.map_err(social_err);
    };
    let ck = format!("mediaid:mal:{mal}");
    if let Some(id) = cached::<Option<u32>>(b, &ck) {
        return Ok(id);
    }
    let id = anilist()
        .anilist_id_of(t.map(|t| t.access_token.as_str()), key)
        .await
        .map_err(social_err)?;
    put(b, &ck, &id, MEDIA_ID_TTL);
    Ok(id)
}

/// Same user name, ignoring case (AniList names are case-insensitive).
fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

// ----- Backend calls -----

pub async fn profile(b: &LiveBackend, name: Option<&str>) -> UiResult<ProfileVm> {
    let name = name.map(str::trim).filter(|n| !n.is_empty());
    match name {
        None => {
            let t = token(b)?;
            let key = cache_key(Some(&t), "profile:me");
            if let Some(p) = cached::<Profile>(b, &key) {
                return Ok(profile_vm(&p, true));
            }
            let p = anilist()
                .viewer_profile(&t.access_token)
                .await
                .map_err(social_err)?;
            put(b, &key, &p, PROFILE_TTL);
            Ok(profile_vm(&p, true))
        }
        Some(name) => {
            let t = optional_token(b);
            let key = cache_key(t.as_ref(), &format!("profile:{}", name.to_ascii_lowercase()));
            let p = match cached::<Profile>(b, &key) {
                Some(p) => p,
                None => {
                    let p = anilist()
                        .user_profile(t.as_ref().map(|t| t.access_token.as_str()), name)
                        .await
                        .map_err(social_err)?
                        .ok_or_else(|| not_found(name))?;
                    put(b, &key, &p, PROFILE_TTL);
                    p
                }
            };
            let is_me = t.as_ref().is_some_and(|t| {
                t.user_id == Some(p.user.id)
                    || t.username.as_deref().is_some_and(|u| same_name(u, &p.user.name))
            });
            Ok(profile_vm(&p, is_me))
        }
    }
}

pub async fn follow(b: &LiveBackend, user_id: u64, follow: bool) -> UiResult<()> {
    let t = token(b)?;
    anilist()
        .set_follow(&t.access_token, user_id, follow)
        .await
        .map_err(social_err)?;
    // Both profiles' counters and flags, and the follow lists, are stale now.
    forget(b, Some(&t), "profile:");
    forget(b, Some(&t), "follow:");
    Ok(())
}

async fn follow_page(b: &LiveBackend, user_id: u64, page: u32, edge: &str) -> UiResult<UserPageVm> {
    let t = optional_token(b);
    let page = page.max(1);
    let key = cache_key(t.as_ref(), &format!("follow:{user_id}:{edge}:{page}"));
    if let Some(p) = cached::<UserPage>(b, &key) {
        return Ok(user_page_vm(&p));
    }
    let tok = t.as_ref().map(|t| t.access_token.as_str());
    let fetched = if edge == "followers" {
        anilist().followers(tok, user_id, page).await
    } else {
        anilist().following(tok, user_id, page).await
    };
    let p = fetched.map_err(social_err)?;
    put(b, &key, &p, FOLLOWS_TTL);
    Ok(user_page_vm(&p))
}

pub async fn followers(b: &LiveBackend, user_id: u64, page: u32) -> UiResult<UserPageVm> {
    follow_page(b, user_id, page, "followers").await
}

pub async fn following(b: &LiveBackend, user_id: u64, page: u32) -> UiResult<UserPageVm> {
    follow_page(b, user_id, page, "following").await
}

pub async fn activity(b: &LiveBackend, feed: &ActivityFeedVm, page: u32) -> UiResult<Vec<ActivityVm>> {
    let (t, core_feed) = match feed {
        ActivityFeedVm::Following => (Some(token(b)?), ActivityFeed::Following),
        ActivityFeedVm::Global => (optional_token(b), ActivityFeed::Global),
        ActivityFeedVm::User(id) => (optional_token(b), ActivityFeed::User(*id)),
        ActivityFeedVm::Media(media_id) => {
            // The viewer's own activity on a title (Details).
            let t = token(b)?;
            let Some(key) = media_key(media_id) else {
                return Ok(Vec::new());
            };
            let Some(anilist_id) = anilist_media_id(b, Some(&t), key).await? else {
                return Ok(Vec::new());
            };
            let me = viewer_id(b, &t).await?;
            (
                Some(t),
                ActivityFeed::Media {
                    media_id: anilist_id,
                    user_id: Some(me),
                },
            )
        }
    };
    let list = anilist()
        .activities(
            t.as_ref().map(|t| t.access_token.as_str()),
            &core_feed,
            page.max(1),
        )
        .await
        .map_err(social_err)?;
    Ok(list.iter().map(activity_vm).collect())
}

pub async fn activity_replies(b: &LiveBackend, activity_id: u64) -> UiResult<Vec<ActivityReplyVm>> {
    let t = optional_token(b);
    let list = anilist()
        .activity_replies(t.as_ref().map(|t| t.access_token.as_str()), activity_id)
        .await
        .map_err(social_err)?;
    Ok(list.iter().map(reply_vm).collect())
}

pub async fn like_activity(b: &LiveBackend, activity_id: u64, like: bool) -> UiResult<()> {
    let t = token(b)?;
    anilist()
        .set_activity_like(&t.access_token, activity_id, like)
        .await
        .map_err(social_err)
}

pub async fn reply(b: &LiveBackend, activity_id: u64, text: &str) -> UiResult<ActivityReplyVm> {
    let t = token(b)?;
    if text.trim().is_empty() {
        return Err(UiError::new(ErrorKind::Other, "empty reply"));
    }
    let r = anilist()
        .save_reply(&t.access_token, activity_id, text)
        .await
        .map_err(social_err)?;
    Ok(reply_vm(&r))
}

pub async fn post_status(b: &LiveBackend, text: &str) -> UiResult<ActivityVm> {
    let t = token(b)?;
    if text.trim().is_empty() {
        return Err(UiError::new(ErrorKind::Other, "empty status"));
    }
    let a = anilist()
        .save_text_activity(&t.access_token, text)
        .await
        .map_err(social_err)?;
    Ok(activity_vm(&a))
}

pub async fn delete_activity(b: &LiveBackend, activity_id: u64) -> UiResult<()> {
    let t = token(b)?;
    anilist()
        .delete_activity(&t.access_token, activity_id)
        .await
        .map_err(social_err)
}

pub async fn notifications(b: &LiveBackend, page: u32, unread_only: bool) -> UiResult<NotificationPageVm> {
    let t = token(b)?;
    let page = page.max(1);
    let api = anilist();
    // The count first: reading the list must not reset it (`resetNotificationCount: false`).
    let unread = api.unread_count(&t.access_token).await.map_err(social_err)?;
    put(b, &cache_key(Some(&t), "unread"), &unread, UNREAD_TTL);
    let p = api
        .notifications(&t.access_token, page)
        .await
        .map_err(social_err)?;
    let offset = (page as usize - 1) * NOTIFICATIONS_PER_PAGE as usize;
    let (items, has_next) = if unread_only {
        let seen_after = offset + p.items.len();
        (
            unread_part(p.items, offset, unread),
            p.has_next && seen_after < unread as usize,
        )
    } else {
        (p.items, p.has_next)
    };
    Ok(NotificationPageVm {
        items: items.iter().map(notification_vm).collect(),
        unread,
        page,
        has_next,
    })
}

/// `Backend::anilist_unread_count`: polled by the rail badge every `Settings::social.notifications_poll_minutes`;
/// cached a minute so other callers share one request.
pub async fn unread_count(b: &LiveBackend) -> UiResult<u32> {
    let t = token(b)?;
    let key = cache_key(Some(&t), "unread");
    if let Some(n) = cached::<u32>(b, &key) {
        return Ok(n);
    }
    let n = anilist()
        .unread_count(&t.access_token)
        .await
        .map_err(social_err)?;
    put(b, &key, &n, UNREAD_TTL);
    Ok(n)
}

pub async fn mark_notifications_read(b: &LiveBackend) -> UiResult<()> {
    let t = token(b)?;
    anilist()
        .reset_notifications(&t.access_token)
        .await
        .map_err(social_err)?;
    put(b, &cache_key(Some(&t), "unread"), &0_u32, UNREAD_TTL);
    Ok(())
}

pub async fn reviews(b: &LiveBackend, media_id: &str, page: u32) -> UiResult<Vec<ReviewVm>> {
    let Some(key) = media_key(media_id) else {
        return Ok(Vec::new());
    };
    let page = page.max(1);
    let ck = format!("reviews:{media_id}:{page}");
    if let Some(p) = cached::<ReviewPage>(b, &ck) {
        return Ok(p.items.iter().map(review_vm).collect());
    }
    let t = optional_token(b);
    let p = anilist()
        .reviews(t.as_ref().map(|t| t.access_token.as_str()), key, page)
        .await
        .map_err(social_err)?;
    put(b, &ck, &p, REVIEWS_TTL);
    Ok(p.items.iter().map(review_vm).collect())
}

pub async fn review(b: &LiveBackend, review_id: u64) -> UiResult<ReviewVm> {
    let ck = format!("review:{review_id}");
    if let Some(r) = cached::<Review>(b, &ck) {
        return Ok(review_vm(&r));
    }
    let t = optional_token(b);
    let r = anilist()
        .review(t.as_ref().map(|t| t.access_token.as_str()), review_id)
        .await
        .map_err(social_err)?;
    put(b, &ck, &r, REVIEW_TTL);
    Ok(review_vm(&r))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::{Media, MediaRef};

    fn tok(user_id: Option<u64>, username: Option<&str>) -> Token {
        Token {
            service: Service::AniList,
            access_token: "x".into(),
            refresh_token: None,
            expires_at: None,
            user_id,
            username: username.map(str::to_owned),
            avatar: None,
        }
    }

    fn media(kind: MediaKind, id: u32) -> Media {
        Media {
            media_ref: MediaRef {
                anilist_id: Some(id),
                mal_id: None,
                kitsu_id: None,
            },
            kind,
            title: format!("T{id}"),
            titles: Default::default(),
            cover: None,
            banner: None,
            color: None,
            synopsis: None,
            genres: Vec::new(),
            episodes: None,
            status: None,
            season: None,
            year: None,
            score: None,
            source_provider: "anilist".into(),
        }
    }

    #[test]
    fn cache_keys_are_per_account() {
        assert_eq!(cache_key(None, "unread"), "anon|unread");
        assert_eq!(cache_key(Some(&tok(Some(7), Some("Ana"))), "unread"), "7|unread");
        assert_eq!(cache_key(Some(&tok(None, Some("Ana"))), "unread"), "Ana|unread");
        assert_eq!(cache_key(Some(&tok(None, None)), "x"), "me|x");
    }

    #[test]
    fn media_ids_and_prefixes() {
        assert_eq!(media_key("21"), Some(MediaKey::AniList(21)));
        assert_eq!(media_key("mal:5114"), Some(MediaKey::Mal(5114)));
        assert_eq!(media_key("kitsu:7"), None);
        assert_eq!(media_key("ext:abc:1"), None);
        let anime = media_vm(&SocialMedia {
            media: media(MediaKind::Anime, 21),
            format: Some("TV".into()),
        });
        assert_eq!(anime.id, "21");
        let manga = media_vm(&SocialMedia {
            media: media(MediaKind::Manga, 30013),
            format: Some("MANGA".into()),
        });
        assert_eq!(manga.id, "manga:30013");
        let novel = media_vm(&SocialMedia {
            media: media(MediaKind::Manga, 86300),
            format: Some("NOVEL".into()),
        });
        assert_eq!(novel.id, "novel:86300");
    }

    #[test]
    fn errors_map_to_ui_kinds() {
        assert_eq!(social_err(ListError::NotConnected).kind, ErrorKind::Auth);
        assert_eq!(social_err(ListError::Unauthorized).detail, "expired");
        assert_eq!(social_err(ListError::RateLimited).kind, ErrorKind::RateLimited);
        assert_eq!(
            social_err(ListError::Network("x".into())).kind,
            ErrorKind::Offline
        );
        assert_eq!(social_err(ListError::Http(500)).kind, ErrorKind::Other);
        assert!(same_name(" Ana ", "ana"));
        assert!(!same_name("Ana", "Anna"));
    }

    #[test]
    fn me_flags_never_follow_myself() {
        let p = Profile {
            user: SocialUser {
                id: 1,
                name: "a".into(),
                avatar: None,
                site_url: "u".into(),
            },
            banner: None,
            about: None,
            stats: Default::default(),
            followers: 3,
            following: 4,
            is_following: true,
            is_follower: true,
            favourites: Vec::new(),
            created_at: None,
        };
        let me = profile_vm(&p, true);
        assert!(me.is_me && !me.is_following && !me.is_follower);
        assert_eq!((me.followers, me.following), (3, 4));
        let other = profile_vm(&p, false);
        assert!(!other.is_me && other.is_following && other.is_follower);
    }
}
