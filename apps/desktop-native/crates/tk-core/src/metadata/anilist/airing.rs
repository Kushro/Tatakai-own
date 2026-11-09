//! AniList queries of Home / calendar / recommendations (wave B, worker **b**; checklist `home.latest-episodes`,
//! `home.upcoming`, `home.most-favorite`, `home.genre-cloud`, `home.because-you-watched`, `home.calendar.schedule`,
//! `home.followed-updates`, `home.recs.*`). Child module of `anilist`: queries that select `...mediaCard` go through
//! the private `post` of `mod.rs` (it appends the fragment); the others through [`AniList::post_bare`] here (an unused
//! fragment fails GraphQL validation). Top 10 uses worker a's `trending_since` (`Page.mediaTrends`), which is the
//! closest AniList has to a time-boxed ranking.
//!
//! * [`AniList::airing_schedules`]: `Page.airingSchedules` between two instants (optionally of some titles), oldest or
//!   newest first: latest episodes, the calendar week, followed updates, reminders.
//! * [`AniList::genres`]: `GenreCollection`.
//! * [`AniList::upcoming`] (`status: NOT_YET_RELEASED`, by popularity, with the start date) and
//!   [`AniList::most_favourite`] (`sort: FAVOURITES_DESC`).
//! * [`AniList::recommendations_of`]: the community recommendations of one title ("Because you watched").
//! * [`AniList::features`] / [`AniList::discover`]: genres, studios, year and tags of seen titles and of candidate titles
//!   for the local recommendation engine (`crate::recs`).

use serde_json::{Value, json};

use super::AniList;
use super::details::format_fuzzy_date;
use crate::error::ProviderError;
use crate::metadata::mapping::map_anilist_media;
use crate::model::Media;
use crate::recs::{Candidate, MIN_TAG_RANK};

/// Rows per airing page (AniList's maximum).
pub const AIRING_PER_PAGE: u32 = 50;

const AIRING_QUERY: &str = "query ($page: Int, $perPage: Int, $from: Int, $to: Int, $ids: [Int], $sort: [AiringSort]) { \
Page(page: $page, perPage: $perPage) { pageInfo { hasNextPage currentPage } \
airingSchedules(airingAt_greater: $from, airingAt_lesser: $to, mediaId_in: $ids, sort: $sort) { \
episode airingAt media { ...mediaCard isAdult popularity } } } }";

const GENRES_QUERY: &str = "query { GenreCollection }";

const SORTED_QUERY: &str = "query ($page: Int, $perPage: Int, $sort: [MediaSort], $status: MediaStatus, $isAdult: Boolean) { \
Page(page: $page, perPage: $perPage) { pageInfo { hasNextPage currentPage } \
media(type: ANIME, sort: $sort, status: $status, isAdult: $isAdult) { ...mediaCard isAdult startDate { year month day } } } }";

const RECS_QUERY: &str = "query ($id: Int) { Media(id: $id, type: ANIME) { id \
recommendations(page: 1, perPage: 15, sort: [RATING_DESC]) { nodes { rating mediaRecommendation { ...mediaCard isAdult } } } } }";

const FEATURES_QUERY: &str = "query ($ids: [Int], $perPage: Int) { Page(page: 1, perPage: $perPage) { \
media(id_in: $ids, type: ANIME) { id genres seasonYear startDate { year } averageScore \
studios(isMain: true) { nodes { name } } tags { name rank isMediaSpoiler } } } }";

const DISCOVER_QUERY: &str = "query ($page: Int, $genres: [String], $exclude: [Int], $isAdult: Boolean) { \
Page(page: $page, perPage: 50) { pageInfo { hasNextPage currentPage } \
media(type: ANIME, genre_in: $genres, id_not_in: $exclude, sort: [SCORE_DESC], isAdult: $isAdult, popularity_greater: 8000, \
format_not_in: [MUSIC]) { ...mediaCard isAdult startDate { year } studios(isMain: true) { nodes { name } } \
tags { name rank isMediaSpoiler } } } }";

/// One airing slot.
#[derive(Debug, Clone, PartialEq)]
pub struct AiringRow {
    pub media: Media,
    pub episode: u32,
    /// Unix seconds.
    pub at_unix: i64,
    pub adult: bool,
    /// AniList `popularity` (list users), to rank a busy day.
    pub popularity: u32,
}

/// A page of [`AiringRow`]s.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AiringPage {
    pub rows: Vec<AiringRow>,
    pub has_next: bool,
}

/// A title with its start date (`YYYY-MM-DD`, partial dates trimmed) and 18+ flag.
#[derive(Debug, Clone, PartialEq)]
pub struct DatedMedia {
    pub media: Media,
    pub start_date: Option<String>,
    pub adult: bool,
}

fn opt_u32(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn graphql_err(doc: &Value, what: &str) -> ProviderError {
    ProviderError::Parse(doc["errors"][0]["message"].as_str().map_or_else(|| format!("missing {what}"), str::to_owned))
}

fn year_u16(v: &Value) -> Option<u16> {
    v.as_u64().and_then(|y| u16::try_from(y).ok()).filter(|y| *y > 1900)
}

/// `studios.nodes[].name`.
fn studios(m: &Value) -> Vec<String> {
    m["studios"]["nodes"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["name"].as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// Non-spoiler tags with their rank, ranked tags only (`>= MIN_TAG_RANK` are what the engine reads).
fn tags(m: &Value) -> Vec<(String, u8)> {
    let mut v: Vec<(String, u8)> = m["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|t| t["isMediaSpoiler"].as_bool() != Some(true))
                .filter_map(|t| Some((t["name"].as_str()?.to_owned(), u8::try_from(t["rank"].as_u64()?.min(100)).ok()?)))
                .collect()
        })
        .unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

/// Features of one AniList media object (works for the features query and for full cards).
pub fn map_features(m: &Value) -> Option<Candidate> {
    let anilist_id = opt_u32(&m["id"]).filter(|id| *id != 0)?;
    Some(Candidate {
        anilist_id,
        genres: m["genres"].as_array().map(|a| a.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
        studios: studios(m),
        year: year_u16(&m["seasonYear"]).or_else(|| year_u16(&m["startDate"]["year"])),
        tags: tags(m),
        mean_score: opt_u32(&m["averageScore"]),
    })
}

/// `Page.airingSchedules` -> rows (entries without media or episode skipped).
pub fn map_airing_page(doc: &Value) -> Result<AiringPage, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        return Err(graphql_err(doc, "data.Page"));
    }
    let rows = page["airingSchedules"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let m = &s["media"];
                    opt_u32(&m["id"]).filter(|id| *id != 0)?;
                    Some(AiringRow {
                        media: map_anilist_media(m),
                        episode: opt_u32(&s["episode"])?,
                        at_unix: s["airingAt"].as_i64()?,
                        adult: m["isAdult"].as_bool() == Some(true),
                        popularity: opt_u32(&m["popularity"]).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(AiringPage { rows, has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false) })
}

/// `Page.media` with `startDate` -> dated titles + has next.
pub fn map_dated_page(doc: &Value) -> Result<(Vec<DatedMedia>, bool), ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        return Err(graphql_err(doc, "data.Page"));
    }
    let items = page["media"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|m| opt_u32(&m["id"]).is_some_and(|id| id != 0))
                .map(|m| {
                    let d = &m["startDate"];
                    let date = (d["year"].as_i64().and_then(|y| i32::try_from(y).ok()), opt_u32(&d["month"]), opt_u32(&d["day"]));
                    DatedMedia { media: map_anilist_media(m), start_date: format_fuzzy_date(date), adult: m["isAdult"].as_bool() == Some(true) }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((items, page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false)))
}

/// `GenreCollection` -> names in AniList order.
pub fn map_genres(doc: &Value) -> Result<Vec<String>, ProviderError> {
    let list = doc["data"]["GenreCollection"].as_array().ok_or_else(|| graphql_err(doc, "GenreCollection"))?;
    Ok(list.iter().filter_map(|g| g.as_str().map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned)).collect())
}

/// `Media.recommendations` -> recommended titles (best rated first), `(media, adult)`.
pub fn map_recommendations(doc: &Value) -> Result<Vec<(Media, bool)>, ProviderError> {
    let m = &doc["data"]["Media"];
    if m.is_null() {
        // Unknown id: no recommendations (AniList answers 404 + errors).
        return Ok(Vec::new());
    }
    Ok(m["recommendations"]["nodes"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|n| &n["mediaRecommendation"])
                .filter(|r| opt_u32(&r["id"]).is_some_and(|id| id != 0))
                .map(|r| (map_anilist_media(r), r["isAdult"].as_bool() == Some(true)))
                .collect()
        })
        .unwrap_or_default())
}

/// `Page.media` with features -> features per title.
pub fn map_features_page(doc: &Value) -> Result<Vec<Candidate>, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        return Err(graphql_err(doc, "data.Page"));
    }
    Ok(page["media"].as_array().map(|a| a.iter().filter_map(map_features).collect()).unwrap_or_default())
}

/// Discover page -> `(media, features, adult)`.
pub fn map_discover_page(doc: &Value) -> Result<Vec<(Media, Candidate, bool)>, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        return Err(graphql_err(doc, "data.Page"));
    }
    Ok(page["media"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| Some((map_anilist_media(m), map_features(m)?, m["isAdult"].as_bool() == Some(true))))
                .collect()
        })
        .unwrap_or_default())
}

/// Variables of [`AIRING_QUERY`] (pure).
pub fn airing_vars(from_unix: i64, to_unix: i64, ids: Option<&[u32]>, newest_first: bool, page: u32) -> Value {
    let mut v = json!({
        "page": page.max(1),
        "perPage": AIRING_PER_PAGE,
        // `_greater` / `_lesser` are exclusive.
        "from": from_unix.saturating_sub(1).max(0),
        "to": to_unix.saturating_add(1),
        "sort": [if newest_first { "TIME_DESC" } else { "TIME" }],
    });
    if let Some(ids) = ids {
        v["ids"] = json!(ids);
    }
    v
}

impl AniList {
    /// POST without the card fragment (for queries that select no `...mediaCard`).
    async fn post_bare(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        let body: Value = resp.json().await.map_err(|e| ProviderError::Parse(e.without_url().to_string()))?;
        if body["errors"].as_array().is_some_and(|e| e.iter().any(|x| x["status"] == 429)) {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        if !(200..300).contains(&status) && status != 404 {
            return Err(ProviderError::Http(status));
        }
        Ok(body)
    }

    /// Episodes airing in `[from_unix, to_unix]` (of `ids` when given; an empty `ids` answers nothing).
    pub async fn airing_schedules(
        &self,
        from_unix: i64,
        to_unix: i64,
        ids: Option<&[u32]>,
        newest_first: bool,
        page: u32,
    ) -> Result<AiringPage, ProviderError> {
        if ids.is_some_and(<[u32]>::is_empty) || to_unix < from_unix {
            return Ok(AiringPage::default());
        }
        map_airing_page(&self.post(AIRING_QUERY, airing_vars(from_unix, to_unix, ids, newest_first, page)).await?)
    }

    /// AniList's genre list.
    pub async fn genres(&self) -> Result<Vec<String>, ProviderError> {
        map_genres(&self.post_bare(GENRES_QUERY, json!({})).await?)
    }

    async fn sorted(&self, sort: &str, status: Option<&str>, page: u32, adult: bool) -> Result<(Vec<DatedMedia>, bool), ProviderError> {
        let mut v = json!({ "page": page.max(1), "perPage": 20, "sort": [sort] });
        if let Some(s) = status {
            v["status"] = s.into();
        }
        if !adult {
            v["isAdult"] = false.into();
        }
        map_dated_page(&self.post(SORTED_QUERY, v).await?)
    }

    /// Not yet released titles, most popular first, with their expected start date.
    pub async fn upcoming(&self, page: u32, adult: bool) -> Result<(Vec<DatedMedia>, bool), ProviderError> {
        self.sorted("POPULARITY_DESC", Some("NOT_YET_RELEASED"), page, adult).await
    }

    /// Most favourited titles of all time.
    pub async fn most_favourite(&self, page: u32, adult: bool) -> Result<(Vec<DatedMedia>, bool), ProviderError> {
        self.sorted("FAVOURITES_DESC", None, page, adult).await
    }

    /// Community recommendations of `anilist_id`, best rated first.
    pub async fn recommendations_of(&self, anilist_id: u32) -> Result<Vec<(Media, bool)>, ProviderError> {
        map_recommendations(&self.post(RECS_QUERY, json!({ "id": anilist_id })).await?)
    }

    /// Genres / studios / year / tags of up to 50 titles.
    pub async fn features(&self, ids: &[u32]) -> Result<Vec<Candidate>, ProviderError> {
        let ids: Vec<u32> = ids.iter().copied().filter(|i| *i != 0).take(50).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        map_features_page(&self.post_bare(FEATURES_QUERY, json!({ "ids": ids, "perPage": ids.len() })).await?)
    }

    /// Well rated titles of any of `genres`, without `exclude`, with their features (recommendation candidates).
    pub async fn discover(
        &self,
        genres: &[String],
        exclude: &[u32],
        page: u32,
        adult: bool,
    ) -> Result<Vec<(Media, Candidate, bool)>, ProviderError> {
        let mut v = json!({ "page": page.max(1), "exclude": exclude.iter().take(500).collect::<Vec<_>>() });
        if !genres.is_empty() {
            v["genres"] = json!(genres);
        }
        if !adult {
            v["isAdult"] = false.into();
        }
        map_discover_page(&self.post(DISCOVER_QUERY, v).await?)
    }
}

/// Tags the recommendation engine would read (re-export of the rank threshold for tk-app's own mapping).
pub const TAG_RANK_THRESHOLD: u8 = MIN_TAG_RANK;

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: u32, adult: bool) -> Value {
        json!({ "id": id, "idMal": id + 1, "type": "ANIME", "title": { "romaji": format!("T{id}") }, "isAdult": adult,
            "genres": ["Action"], "seasonYear": 2020, "averageScore": 81, "popularity": 1200,
            "studios": { "nodes": [{ "name": "MAPPA" }] },
            "tags": [{ "name": "Gore", "rank": 40, "isMediaSpoiler": false }, { "name": "Twist", "rank": 95, "isMediaSpoiler": true },
                     { "name": "Shounen", "rank": 88, "isMediaSpoiler": false }],
            "startDate": { "year": 2027, "month": 4, "day": null } })
    }

    #[test]
    fn maps_airing_pages() {
        let doc = json!({ "data": { "Page": { "pageInfo": { "hasNextPage": true }, "airingSchedules": [
            { "episode": 5, "airingAt": 1_800_000_000_i64, "media": card(1, false) },
            { "episode": 6, "airingAt": 1_800_000_100_i64, "media": card(2, true) },
            { "episode": null, "airingAt": 1, "media": card(3, false) },
            { "episode": 1, "airingAt": 1, "media": null }
        ] } } });
        let p = map_airing_page(&doc).unwrap();
        assert!(p.has_next);
        assert_eq!(p.rows.len(), 2);
        assert_eq!((p.rows[0].episode, p.rows[0].at_unix, p.rows[0].adult, p.rows[0].popularity), (5, 1_800_000_000, false, 1200));
        assert!(p.rows[1].adult);
        assert!(map_airing_page(&json!({ "errors": [{ "message": "boom" }] })).is_err());
    }

    #[test]
    fn airing_vars_widen_the_exclusive_bounds() {
        let v = airing_vars(100, 200, Some(&[1, 2]), true, 0);
        assert_eq!((v["from"].as_i64(), v["to"].as_i64(), v["page"].as_u64()), (Some(99), Some(201), Some(1)));
        assert_eq!(v["sort"], json!(["TIME_DESC"]));
        assert_eq!(v["ids"], json!([1, 2]));
        assert!(airing_vars(0, 1, None, false, 2).get("ids").is_none());
    }

    #[test]
    fn maps_dated_genres_recs_and_features() {
        let doc = json!({ "data": { "Page": { "pageInfo": { "hasNextPage": false }, "media": [card(7, false)] } } });
        let (items, next) = map_dated_page(&doc).unwrap();
        assert!(!next);
        assert_eq!(items[0].start_date.as_deref(), Some("2027-04"));

        let g = map_genres(&json!({ "data": { "GenreCollection": ["Action", " ", "Drama"] } })).unwrap();
        assert_eq!(g, vec!["Action".to_owned(), "Drama".to_owned()]);

        let r = map_recommendations(&json!({ "data": { "Media": { "id": 1, "recommendations": { "nodes": [
            { "rating": 10, "mediaRecommendation": card(9, true) }, { "rating": 1, "mediaRecommendation": null }
        ] } } } }))
        .unwrap();
        assert_eq!(r.len(), 1);
        assert!(r[0].1);
        assert!(map_recommendations(&json!({ "data": { "Media": null } })).unwrap().is_empty());

        let f = map_features(&card(4, false)).unwrap();
        assert_eq!(f.anilist_id, 4);
        assert_eq!(f.studios, vec!["MAPPA".to_owned()]);
        assert_eq!(f.year, Some(2020));
        // spoiler tags dropped, highest rank first
        assert_eq!(f.tags, vec![("Shounen".to_owned(), 88), ("Gore".to_owned(), 40)]);
        assert_eq!(f.mean_score, Some(81));
        let d = map_discover_page(&json!({ "data": { "Page": { "media": [card(4, true)] } } })).unwrap();
        assert_eq!((d[0].1.anilist_id, d[0].2), (4, true));
    }
}
