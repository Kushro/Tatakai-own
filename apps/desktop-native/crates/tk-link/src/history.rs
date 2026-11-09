//! Watch-history sync between the user's own devices (wave A, worker **f**; plan 14 section 0.1 #9,
//! docs/redesign/15-parity-wave-a-contract.md section 3.8). Replaces the removed Supabase `watch_history` table:
//! the PC is the server, the phone (`apps/android-native/core/link/LinkHistoryClient.kt`, worker q) the client,
//! over the existing mTLS Link.
//!
//! CANONICAL WIRE (shared with the Android DTOs; neither side changes it alone):
//! * Row = the camelCase JSON of `tk_core::HistoryEntry` (`animeId`, `title`, `poster?`, `episodeId`, `episode`,
//!   `progressSec`, `durationSec?`, `completed`, `watchedAt` RFC 3339, `anilistId?`, `malId?`).
//! * `GET /v1/history?since=<unix seconds, exclusive; absent/0 = all>&limit=<1..=MAX_ROWS>` -> [`HistoryPage`]:
//!   rows with `watchedAt > since`, **oldest first**, at most `limit`. `next_since_unix` = `watchedAt` of the last
//!   row (or the requested `since` when the page is empty); with `truncated` the phone asks again from it.
//! * `POST /v1/history` with [`HistoryPush`] (<= [`MAX_BODY`] bytes, 1..=[`MAX_ROWS`] rows) -> [`HistoryPushResult`]:
//!   the PC merges the rows (`LinkHost::history_merge`: newest `watchedAt` wins, tie -> higher progress, i.e.
//!   `tk_core::HistoryEntry::wins_over`) and answers how many entered (`merged`).
//! * `DELETE /v1/history` -> 204: the PC drops its whole history (`LinkHost::history_clear`; the phone calls it
//!   only after the user confirmed "also on the PC").
//!
//! Known limit (documented, accepted): the cursor is `watchedAt`, so a row a phone pushes with an old `watchedAt`
//! is not re-served to a second phone's incremental pull (its first full pull gets it). Worker f may move the
//! cursor to a PC-side modification time later without changing the wire.
//!
//! The position is exact seconds (`progressSec`): "resume where you left off on the other device". No user ids:
//! the device certificate is the identity.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tk_core::HistoryEntry;

/// Most rows per page / per push (= the Android client's `LinkClient.MAX_HISTORY_ROWS`).
pub const MAX_ROWS: usize = 500;
/// Body cap of `POST /v1/history` (500 rows of ~400 bytes with headroom).
pub const MAX_BODY: usize = 256 * 1024;

/// One row on the wire (camelCase, same keys as `tk_core::HistoryEntry`). `watchedAt` is UTC RFC 3339
/// (`2026-10-27T21:04:11Z` from the PC; the phone may send millis, `...11.000Z`). `malId` / `anilistId` / `poster`
/// / `durationSec` are omitted when unknown so a merge never overwrites a known value with null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRow {
    pub anime_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster: Option<String>,
    pub episode_id: String,
    pub episode: u32,
    #[serde(default)]
    pub progress_sec: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<u32>,
    #[serde(default)]
    pub completed: bool,
    pub watched_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anilist_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mal_id: Option<u32>,
}

impl From<&HistoryEntry> for HistoryRow {
    fn from(e: &HistoryEntry) -> Self {
        Self {
            anime_id: e.anime_id.clone(),
            title: e.title.clone(),
            poster: e.poster.clone().filter(|p| !p.is_empty()),
            episode_id: e.episode_id.clone(),
            episode: e.episode,
            progress_sec: e.progress_sec,
            duration_sec: e.duration_sec.filter(|d| *d > 0),
            completed: e.completed,
            watched_at: e.watched_at.to_offset(time::UtcOffset::UTC).format(&Rfc3339).unwrap_or_default(),
            anilist_id: e.anilist_id,
            mal_id: e.mal_id,
        }
    }
}

impl HistoryRow {
    /// `None` when `watchedAt` does not parse or the row has neither an AniList nor a MAL id (E9: never stored).
    pub fn into_entry(self) -> Option<HistoryEntry> {
        if self.anilist_id.is_none() && self.mal_id.is_none() {
            return None;
        }
        let watched_at = OffsetDateTime::parse(&self.watched_at, &Rfc3339).ok()?;
        Some(HistoryEntry {
            anime_id: self.anime_id,
            title: self.title,
            poster: self.poster.filter(|p| !p.is_empty()),
            episode_id: self.episode_id,
            episode: self.episode,
            progress_sec: self.progress_sec,
            duration_sec: self.duration_sec.filter(|d| *d > 0),
            completed: self.completed,
            watched_at,
            anilist_id: self.anilist_id,
            mal_id: self.mal_id,
        })
    }
}

/// `GET /v1/history` answer (= Android `LinkHistoryPageDto`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    /// Oldest first.
    pub items: Vec<HistoryRow>,
    /// `watchedAt` (unix seconds) of the last item, or the requested `since` when `items` is empty: the phone's
    /// next cursor.
    pub next_since_unix: i64,
    /// More rows matched than `limit`: ask again from `next_since_unix`.
    #[serde(default)]
    pub truncated: bool,
}

/// `POST /v1/history` body (= Android `LinkHistoryPushDto`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPush {
    pub items: Vec<HistoryRow>,
}

/// `POST /v1/history` answer (= Android `LinkHistoryPushResultDto`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPushResult {
    /// Rows the PC stored (newer than what it had, or new). Rows it already had newer are not counted.
    pub merged: usize,
}

/// Build the page from `rows` already filtered to `watchedAt > since` and ordered oldest first (what
/// `LinkHost::history_since` returns, asked with `limit + 1`). More than `limit` rows marks the page truncated.
pub fn page(rows: Vec<HistoryEntry>, limit: usize, since_unix: i64) -> HistoryPage {
    let limit = limit.clamp(1, MAX_ROWS);
    let truncated = rows.len() > limit;
    let items: Vec<HistoryRow> = rows.iter().take(limit).map(HistoryRow::from).collect();
    let next_since_unix = rows.iter().take(limit).last().map(|e| e.watched_at.unix_timestamp()).unwrap_or(since_unix);
    HistoryPage { items, next_since_unix, truncated }
}

/// Clamp the `limit` query parameter (absent = `MAX_ROWS`).
pub fn clamp_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(MAX_ROWS).clamp(1, MAX_ROWS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::MediaRef;

    fn row(ep: u32, at: i64) -> HistoryEntry {
        HistoryEntry::for_playback(&MediaRef::anilist(21), "One Piece", None, ep, 812, Some(1440), OffsetDateTime::from_unix_timestamp(at).unwrap()).unwrap()
    }

    #[test]
    fn page_keeps_order_cursor_and_flags_truncation() {
        let p = page(vec![row(1, 10), row(2, 20), row(3, 30)], 2, 5);
        assert_eq!(p.items.iter().map(|r| r.episode).collect::<Vec<_>>(), [1, 2]);
        assert!(p.truncated);
        assert_eq!(p.next_since_unix, 20, "cursor = watchedAt of the last item served");
        let empty = page(Vec::new(), 2, 77);
        assert_eq!((empty.next_since_unix, empty.truncated), (77, false), "empty page keeps the requested cursor");
        assert_eq!(clamp_limit(None), MAX_ROWS);
        assert_eq!(clamp_limit(Some(0)), 1);
        assert_eq!(clamp_limit(Some(10_000)), MAX_ROWS);
    }

    #[test]
    fn rows_use_the_history_entry_camel_case_shape_and_round_trip() {
        let json = serde_json::to_value(page(vec![row(3, 1_793_135_051)], 10, 0)).unwrap();
        assert!(json.get("next_since_unix").is_some() && json.get("truncated").is_some());
        let item = &json["items"][0];
        assert_eq!(item["animeId"], "21");
        assert_eq!(item["title"], "One Piece");
        assert_eq!(item["episodeId"], "21?ep=3");
        assert_eq!(item["episode"], 3);
        assert_eq!(item["progressSec"], 812);
        assert_eq!(item["durationSec"], 1440);
        assert_eq!(item["watchedAt"], "2026-10-27T21:04:11Z");
        assert_eq!(item["anilistId"], 21);
        assert!(item.get("malId").is_none() && item.get("poster").is_none(), "unknown ids / poster are omitted");
        // a phone row with millis and only a MAL id parses back into the same entry shape
        let push: HistoryPush = serde_json::from_str(
            r#"{"items":[{"animeId":"mal:52991","title":"x","episodeId":"mal:52991?ep=1","episode":1,"progressSec":90,"durationSec":100,"completed":true,"watchedAt":"2026-10-27T21:04:11.000Z","malId":52991}]}"#,
        )
        .unwrap();
        let e = push.items.into_iter().next().unwrap().into_entry().unwrap();
        assert_eq!((e.anime_id.as_str(), e.episode, e.progress_sec, e.mal_id), ("mal:52991", 1, 90, Some(52991)));
        assert_eq!(e.watched_at.unix_timestamp(), 1_793_135_051);
        assert!(serde_json::from_str::<HistoryPush>(r#"{"items":[],"user_id":"x"}"#).is_err());
        assert_eq!(serde_json::to_value(HistoryPushResult { merged: 2 }).unwrap(), serde_json::json!({ "merged": 2 }));
        // rows without any id are refused (E9)
        let r = HistoryRow { anilist_id: None, mal_id: None, ..HistoryRow::from(&row(1, 1)) };
        assert!(r.into_entry().is_none());
    }
    // ---- F6 (wave A gaps) ----

    #[test]
    fn exact_limit_is_not_truncated_and_limit_zero_serves_one() {
        let p = page(vec![row(1, 10), row(2, 20)], 2, 0);
        assert!(!p.truncated);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.next_since_unix, 20);
        let one = page(vec![row(1, 10), row(2, 20)], 0, 0);
        assert_eq!(one.items.len(), 1);
        assert!(one.truncated);
        assert_eq!(one.next_since_unix, 10);
    }

    #[test]
    fn unparsable_watched_at_is_refused() {
        let r = HistoryRow { watched_at: "yesterday".into(), ..HistoryRow::from(&row(1, 1)) };
        assert!(r.into_entry().is_none());
        let r = HistoryRow { watched_at: String::new(), ..HistoryRow::from(&row(1, 1)) };
        assert!(r.into_entry().is_none());
    }

    #[test]
    fn empty_poster_and_zero_duration_are_unknown_both_ways() {
        let mut e = row(4, 1_000);
        e.poster = Some(String::new());
        e.duration_sec = Some(0);
        let r = HistoryRow::from(&e);
        assert_eq!((r.poster.as_ref(), r.duration_sec), (None, None));
        let v = serde_json::to_value(&r).unwrap();
        assert!(v.get("poster").is_none() && v.get("durationSec").is_none());
        let back = HistoryRow { poster: Some(String::new()), duration_sec: Some(0), ..r }.into_entry().unwrap();
        assert_eq!((back.poster, back.duration_sec), (None, None));
    }

    #[test]
    fn offsets_are_written_as_utc_and_read_back_to_the_same_instant() {
        let mut e = row(1, 1_793_135_051);
        e.watched_at = e.watched_at.to_offset(time::UtcOffset::from_hms(-3, 0, 0).unwrap());
        let r = HistoryRow::from(&e);
        assert_eq!(r.watched_at, "2026-10-27T21:04:11Z");
        let parsed: HistoryRow = serde_json::from_str(
            r#"{"animeId":"21","title":"x","episodeId":"21?ep=1","episode":1,"watchedAt":"2026-10-27T18:04:11-03:00","anilistId":21}"#,
        )
        .unwrap();
        assert_eq!((parsed.progress_sec, parsed.completed), (0, false), "defaults");
        assert_eq!(parsed.into_entry().unwrap().watched_at.unix_timestamp(), 1_793_135_051);
    }

    #[test]
    fn page_wire_defaults_truncated() {
        let p: HistoryPage = serde_json::from_str(r#"{"items":[],"next_since_unix":5}"#).unwrap();
        assert_eq!(p, HistoryPage { items: Vec::new(), next_since_unix: 5, truncated: false });
    }
}
