//! AniList social queries (wave B, worker **n**; plan 14 section 0.1 #10; checklist `social.*`, `details.reviews`).
//! Child module of `anilist` (private `post`, authenticated with the token from `Secrets`). Planned:
//! `AniList::viewer_profile()` / `user_profile(name)` (`User { about bannerImage statistics { anime manga } favourites
//! isFollowing isFollower }`), `toggle_follow(user_id)`, `followers(user_id, page)` / `following(user_id, page)`,
//! `activities(feed, page)` (`Page.activities(isFollowing / userId / mediaId, sort: ID_DESC)` with the
//! `ListActivity` / `TextActivity` / `MessageActivity` fragments), `activity_replies(id)`, `toggle_like(id,
//! ACTIVITY | ACTIVITY_REPLY)`, `save_reply(activity_id, text)`, `save_text_activity(text)`, `delete_activity(id)`,
//! `notifications(page, reset)` (`Page.notifications(resetNotificationCount:)` with the per-type fragments),
//! `unread_count()` (`Viewer.unreadNotificationCount`), `reviews(media_id, page)` (`Page.reviews(mediaId, sort:
//! RATING_DESC)`), `review(id)` (`Review { body(asHtml: false) }`).

