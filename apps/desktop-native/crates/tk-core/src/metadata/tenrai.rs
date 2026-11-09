//! Tenrai provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.tenrai`; classic
//! `lib/metadata/providers/tenrai.ts`, `lib/tenrai/client.ts`). Last-resort source for titles AniList / Kitsu do not
//! have. Alive on 2026-11-09 (API 1.0.30, checked against `https://api.tenrai.org/documentation/openapi.json`, the
//! old prose docs are stale): base `https://api.tenrai.org/v1`, Jikan v4 schema (`{ data, pagination }`), keyed by
//! **MAL id** (AniList `idMal`). Public, no key: 120 req/min, 4 req/s per IP; a `429` answers `RateLimited` (router
//! cool-down) and, with `Retry-After`, this provider also refuses to call again until that time has passed (the
//! router's own cool-down is a fixed 60 s). The private `X-Server-Key` is never sent.
//!
//! Routes: `/anime/{id}/full` (`media`, [`Tenrai::facts`]: synopsis, score, rank, `broadcast`), `/anime/{id}/episodes`
//! (`episodes`, titles per episode, paginated), `/anime?q=` (`search`), `/top/anime` and `/seasons/now` (`feed`),
//! `/schedules` ([`Tenrai::schedules`], fallback of the airing calendar). Every list call sends `sfw=true`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use super::mal::{ExternalFacts, opt_str, opt_u32, push_unique, score_100, status_err};
use crate::error::ProviderError;
use crate::model::{Episode, FeedKind, FeedPage, Media, MediaKind, MediaRef};

pub const TENRAI_API_BASE: &str = "https://api.tenrai.org/v1";
const PAGE_SIZE: u32 = 24;
/// Episode pages fetched at most (100 episodes each).
const EPISODE_PAGES: u32 = 20;
/// Longest `Retry-After` honoured.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(600);

/// One weekly broadcast slot of `/schedules` (`day` = `Mondays`.. as Jikan spells it, `time` = `HH:MM` in `timezone`).
#[derive(Debug, Clone, PartialEq)]
pub struct Broadcast {
    pub media: Media,
    /// 0 = Monday .. 6 = Sunday.
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    /// IANA zone of `hour:minute` (Tenrai: always `Asia/Tokyo`).
    pub timezone: String,
}

/// Jikan status -> AniList spelling.
fn status(s: &str) -> Option<String> {
    Some(
        match s {
            "Currently Airing" | "Publishing" => "RELEASING",
            "Finished Airing" | "Finished" => "FINISHED",
            "Not yet aired" | "Not yet published" => "NOT_YET_RELEASED",
            "On Hiatus" => "HIATUS",
            "Discontinued" => "CANCELLED",
            _ => return None,
        }
        .to_owned(),
    )
}

/// `Mondays` / `monday` -> 0 .. 6.
pub fn weekday_of(day: &str) -> Option<u8> {
    let d = day.trim().to_ascii_lowercase();
    let d = d.strip_suffix('s').unwrap_or(&d);
    [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ]
    .iter()
    .position(|x| *x == d)
    .map(|i| i as u8)
}

/// `"23:30"` -> `(23, 30)`.
fn parse_hm(s: &str) -> Option<(u8, u8)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.parse::<u8>().ok()?, m.parse::<u8>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// One Jikan v4 anime / manga entry -> [`Media`] (`None` without `mal_id`).
pub fn map_jikan_entry(e: &Value, kind: MediaKind) -> Option<Media> {
    let id = opt_u32(&e["mal_id"])?;
    let mut titles = std::collections::BTreeMap::new();
    if let Some(t) = opt_str(&e["title"]) {
        titles.insert("romaji".to_owned(), t);
    }
    if let Some(t) = opt_str(&e["title_english"]) {
        titles.insert("english".to_owned(), t);
    }
    if let Some(t) = opt_str(&e["title_japanese"]) {
        titles.insert("native".to_owned(), t);
    }
    let title = titles
        .get("english")
        .or_else(|| titles.get("romaji"))
        .cloned()
        .unwrap_or_default();
    let img = &e["images"];
    let year = e["year"]
        .as_i64()
        .or_else(|| e["aired"]["prop"]["from"]["year"].as_i64())
        .or_else(|| e["published"]["prop"]["from"]["year"].as_i64())
        .and_then(|y| i32::try_from(y).ok());
    Some(Media {
        media_ref: MediaRef {
            mal_id: Some(id),
            ..MediaRef::default()
        },
        kind,
        title,
        titles,
        cover: opt_str(&img["webp"]["large_image_url"])
            .or_else(|| opt_str(&img["jpg"]["large_image_url"]))
            .or_else(|| opt_str(&img["jpg"]["image_url"])),
        banner: None,
        color: None,
        synopsis: opt_str(&e["synopsis"]),
        genres: e["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| opt_str(&g["name"])).collect())
            .unwrap_or_default(),
        episodes: opt_u32(&e["episodes"]).or_else(|| opt_u32(&e["chapters"])),
        status: e["status"].as_str().and_then(status),
        season: opt_str(&e["season"]).map(|s| s.to_ascii_uppercase()),
        year,
        score: score_100(&e["score"]),
        source_provider: "tenrai".into(),
    })
}

/// `/anime/{id}/full` `data` -> [`ExternalFacts`].
pub fn map_jikan_facts(e: &Value) -> ExternalFacts {
    let mut titles = Vec::new();
    push_unique(&mut titles, opt_str(&e["title_english"]));
    push_unique(&mut titles, opt_str(&e["title_japanese"]));
    for s in e["title_synonyms"].as_array().into_iter().flatten() {
        push_unique(&mut titles, opt_str(s));
    }
    ExternalFacts {
        provider: "tenrai",
        score: score_100(&e["score"]),
        rank: opt_u32(&e["rank"]),
        popularity_rank: opt_u32(&e["popularity"]),
        members: opt_u32(&e["members"]),
        synopsis: opt_str(&e["synopsis"]),
        titles,
        screenshots: Vec::new(),
        episodes: opt_u32(&e["episodes"]),
    }
}

/// `{ data: [...], pagination }` -> page (entries without id skipped, duplicates dropped).
fn map_list(doc: &Value, kind: MediaKind, page: u32) -> FeedPage {
    let mut seen = std::collections::HashSet::new();
    let items = doc["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| map_jikan_entry(e, kind))
                .filter(|m| m.media_ref.mal_id.is_some_and(|id| seen.insert(id)))
                .collect()
        })
        .unwrap_or_default();
    FeedPage {
        items,
        page,
        has_next: doc["pagination"]["has_next_page"].as_bool().unwrap_or(false),
    }
}

/// `/anime/{id}/episodes` page -> episodes (`mal_id` is the episode number).
pub fn map_jikan_episodes(doc: &Value) -> Vec<Episode> {
    doc["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    Some(Episode {
                        number: opt_u32(&e["mal_id"])?,
                        title: opt_str(&e["title"]).or_else(|| opt_str(&e["title_romanji"])),
                        image: None,
                        air_date: opt_str(&e["aired"]).map(|d| d.chars().take(10).collect()),
                        is_filler: e["filler"].as_bool().unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `/schedules` page -> broadcast slots (entries without a parseable day / time are skipped).
pub fn map_schedules(doc: &Value) -> Vec<Broadcast> {
    doc["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let b = &e["broadcast"];
                    let weekday = b["day"].as_str().and_then(weekday_of)?;
                    let (hour, minute) = b["time"].as_str().and_then(parse_hm)?;
                    Some(Broadcast {
                        media: map_jikan_entry(e, MediaKind::Anime)?,
                        weekday,
                        hour,
                        minute,
                        timezone: opt_str(&b["timezone"]).unwrap_or_else(|| "Asia/Tokyo".to_owned()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub struct Tenrai {
    client: reqwest::Client,
    base: String,
    /// Set from a `429` `Retry-After`: no call before this instant.
    blocked_until: Mutex<Option<Instant>>,
}

impl Tenrai {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            base: TENRAI_API_BASE.to_owned(),
            blocked_until: Mutex::new(None),
        }
    }

    /// Point the provider at another server (tests).
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    fn blocked(&self) -> bool {
        self.blocked_until
            .lock()
            .map(|b| b.is_some_and(|t| Instant::now() < t))
            .unwrap_or(false)
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Option<Value>, ProviderError> {
        if self.blocked() {
            return Err(ProviderError::RateLimited {
                provider: "tenrai".into(),
            });
        }
        let resp = self
            .client
            .get(format!("{}{path}", self.base))
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;
        let code = resp.status().as_u16();
        if code == 429 {
            let wait = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|s| Duration::from_secs(s).min(MAX_RETRY_AFTER));
            if let (Some(wait), Ok(mut b)) = (wait, self.blocked_until.lock()) {
                *b = Some(Instant::now() + wait);
            }
        }
        if code == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&code) {
            return Err(status_err("tenrai", code));
        }
        resp.json::<Value>()
            .await
            .map(Some)
            .map_err(|e| ProviderError::Parse(e.without_url().to_string()))
    }

    /// Synopsis, score, rank and alternative titles of a MAL id (`None` = unknown id).
    pub async fn facts(&self, mal_id: u32) -> Result<Option<ExternalFacts>, ProviderError> {
        Ok(self
            .get(&format!("/anime/{mal_id}/full"), &[])
            .await?
            .map(|d| map_jikan_facts(&d["data"])))
    }

    /// One page of the weekly broadcast schedule (`day` = `monday`.. or `None` for every day), SFW only.
    pub async fn schedules(
        &self,
        day: Option<&str>,
        page: u32,
    ) -> Result<(Vec<Broadcast>, bool), ProviderError> {
        let mut q = vec![
            ("sfw", "true".to_owned()),
            ("page", page.max(1).to_string()),
            ("limit", "25".to_owned()),
        ];
        if let Some(d) = day {
            q.push(("filter", d.to_owned()));
        }
        let doc = self.get("/schedules", &q).await?.unwrap_or(Value::Null);
        Ok((
            map_schedules(&doc),
            doc["pagination"]["has_next_page"].as_bool().unwrap_or(false),
        ))
    }

    fn path(kind: MediaKind) -> &'static str {
        if kind == MediaKind::Manga {
            "manga"
        } else {
            "anime"
        }
    }
}

#[async_trait]
impl MetadataProvider for Tenrai {
    fn id(&self) -> &'static str {
        "tenrai"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime]
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        let page = page.max(1);
        let base = vec![
            ("sfw", "true".to_owned()),
            ("page", page.to_string()),
            ("limit", PAGE_SIZE.to_string()),
        ];
        let with = |extra: &[(&'static str, &str)]| {
            let mut q = base.clone();
            q.extend(extra.iter().map(|(a, b)| (*a, (*b).to_owned())));
            q
        };
        let (path, q) = match f {
            FeedKind::Trending => ("/top/anime".to_owned(), with(&[("filter", "airing")])),
            FeedKind::Popular => ("/top/anime".to_owned(), with(&[("filter", "bypopularity")])),
            FeedKind::TopRated => ("/top/anime".to_owned(), with(&[])),
            FeedKind::Upcoming => ("/top/anime".to_owned(), with(&[("filter", "upcoming")])),
            FeedKind::Airing => (
                "/anime".to_owned(),
                with(&[("status", "airing"), ("order_by", "members"), ("sort", "desc")]),
            ),
            FeedKind::Seasonal => ("/seasons/now".to_owned(), with(&[])),
            FeedKind::RecentlyCompleted => (
                "/anime".to_owned(),
                with(&[("status", "complete"), ("order_by", "end_date"), ("sort", "desc")]),
            ),
        };
        let doc = self.get(&path, &q).await?.unwrap_or(Value::Null);
        Ok(map_list(&doc, k, page))
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let page = page.max(1);
        let q = q.trim();
        if q.is_empty() {
            return Ok(FeedPage {
                items: Vec::new(),
                page,
                has_next: false,
            });
        }
        let query = [
            ("q", q.chars().take(100).collect::<String>()),
            ("sfw", "true".to_owned()),
            ("page", page.to_string()),
            ("limit", PAGE_SIZE.to_string()),
        ];
        let doc = self
            .get(&format!("/{}", Self::path(k)), &query)
            .await?
            .unwrap_or(Value::Null);
        Ok(map_list(&doc, k, page))
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        let Some(id) = r.mal_id.filter(|i| *i > 0) else {
            return Ok(None);
        };
        let doc = self.get(&format!("/{}/{id}/full", Self::path(k)), &[]).await?;
        Ok(doc
            .as_ref()
            .and_then(|d| map_jikan_entry(&d["data"], k))
            .map(|mut m| {
                m.media_ref.anilist_id = r.anilist_id;
                m.media_ref.kitsu_id = r.kitsu_id;
                m
            }))
    }

    async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        let Some(id) = r.mal_id.filter(|i| *i > 0) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<Episode> = Vec::new();
        for page in 1..=EPISODE_PAGES {
            let Some(doc) = self
                .get(&format!("/anime/{id}/episodes"), &[("page", page.to_string())])
                .await?
            else {
                break;
            };
            out.extend(map_jikan_episodes(&doc));
            if !doc["pagination"]["has_next_page"].as_bool().unwrap_or(false) {
                break;
            }
        }
        out.sort_by_key(|e| e.number);
        out.dedup_by_key(|e| e.number);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry() -> Value {
        json!({
            "mal_id": 21, "title": "One Piece", "title_english": "One Piece English", "title_japanese": "ONE PIECE",
            "title_synonyms": ["OP"], "images": { "jpg": { "image_url": "s.jpg", "large_image_url": "l.jpg" } },
            "status": "Currently Airing", "episodes": null, "score": 8.72, "rank": 62, "popularity": 17, "members": 10,
            "synopsis": "Pirates.", "season": "fall", "year": null, "aired": { "prop": { "from": { "year": 1999 } } },
            "genres": [{ "mal_id": 1, "name": "Action" }],
            "broadcast": { "day": "Sundays", "time": "23:15", "timezone": "Asia/Tokyo" }
        })
    }

    #[test]
    fn maps_entries() {
        let m = map_jikan_entry(&entry(), MediaKind::Anime).unwrap();
        assert_eq!(
            m.media_ref,
            MediaRef {
                mal_id: Some(21),
                ..MediaRef::default()
            }
        );
        assert_eq!(m.title, "One Piece English");
        assert_eq!(m.cover.as_deref(), Some("l.jpg"));
        assert_eq!(
            (m.status.as_deref(), m.year, m.season.as_deref()),
            (Some("RELEASING"), Some(1999), Some("FALL"))
        );
        assert_eq!(m.score, Some(87));
        assert_eq!(m.source_provider, "tenrai");
        let f = map_jikan_facts(&entry());
        assert_eq!((f.rank, f.score, f.members), (Some(62), Some(87), Some(10)));
        assert_eq!(
            f.titles,
            vec![
                "One Piece English".to_owned(),
                "ONE PIECE".to_owned(),
                "OP".to_owned()
            ]
        );
    }

    #[test]
    fn maps_lists_without_duplicates() {
        let doc = json!({ "data": [entry(), entry(), { "title": "no id" }], "pagination": { "has_next_page": true } });
        let p = map_list(&doc, MediaKind::Anime, 2);
        assert_eq!((p.items.len(), p.page, p.has_next), (1, 2, true));
    }

    #[test]
    fn maps_schedules_and_episodes() {
        let s = map_schedules(&json!({ "data": [entry(), { "mal_id": 5, "broadcast": { "day": null } }] }));
        assert_eq!(s.len(), 1);
        assert_eq!(
            (s[0].weekday, s[0].hour, s[0].minute, s[0].timezone.as_str()),
            (6, 23, 15, "Asia/Tokyo")
        );
        let e = map_jikan_episodes(&json!({ "data": [
            { "mal_id": 1, "title": "Romance Dawn", "aired": "1999-10-20T00:00:00+00:00", "filler": false },
            { "mal_id": 2, "title": null, "title_romanji": "Ore wa", "filler": true }
        ] }));
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].air_date.as_deref(), Some("1999-10-20"));
        assert_eq!((e[1].title.as_deref(), e[1].is_filler), (Some("Ore wa"), true));
    }

    #[test]
    fn weekdays_and_times() {
        assert_eq!(weekday_of("Mondays"), Some(0));
        assert_eq!(weekday_of("sunday"), Some(6));
        assert_eq!(weekday_of("unknown"), None);
        assert_eq!(parse_hm("01:05"), Some((1, 5)));
        assert_eq!(parse_hm("25:00"), None);
    }

    #[tokio::test]
    async fn refuses_calls_while_retry_after_runs() {
        let t = Tenrai::new(reqwest::Client::new()).with_base("http://127.0.0.1:9");
        *t.blocked_until.lock().unwrap() = Some(Instant::now() + Duration::from_secs(30));
        let r = MediaRef {
            mal_id: Some(1),
            ..MediaRef::default()
        };
        assert_eq!(
            t.media(MediaKind::Anime, &r).await,
            Err(ProviderError::RateLimited {
                provider: "tenrai".into()
            })
        );
        // without a MAL id nothing is asked at all
        assert_eq!(t.media(MediaKind::Anime, &MediaRef::anilist(1)).await, Ok(None));
    }
}
