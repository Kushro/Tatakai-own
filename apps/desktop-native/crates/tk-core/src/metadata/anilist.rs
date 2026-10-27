//! AniList GraphQL provider (anime + manga catalogue; no episode list).

use async_trait::async_trait;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::MetadataProvider;
use super::mapping::{map_anilist_media, map_anilist_page};
use crate::error::ProviderError;
use crate::model::{FeedKind, FeedPage, Media, MediaKind, MediaRef};

const ENDPOINT: &str = "https://graphql.anilist.co";

const CARD_FRAGMENT: &str = "fragment mediaCard on Media { id idMal type format status season seasonYear episodes \
title { romaji english native } coverImage { extraLarge large medium color } bannerImage averageScore genres description(asHtml: false) }";

const PAGE_QUERY: &str = "query ($page: Int, $perPage: Int, $type: MediaType, $sort: [MediaSort], $search: String, \
$format_not_in: [MediaFormat], $status_in: [MediaStatus], $season: MediaSeason, $seasonYear: Int, $isAdult: Boolean, \
$startDate_greater: FuzzyDateInt, $startDate_lesser: FuzzyDateInt) { Page(page: $page, perPage: $perPage) { \
pageInfo { total currentPage lastPage hasNextPage perPage } media(type: $type, sort: $sort, search: $search, \
format_not_in: $format_not_in, status_in: $status_in, season: $season, seasonYear: $seasonYear, isAdult: $isAdult, \
startDate_greater: $startDate_greater, startDate_lesser: $startDate_lesser) { ...mediaCard } } }";

const MEDIA_QUERY: &str = "query ($id: Int, $idMal: Int, $type: MediaType) { Media(id: $id, idMal: $idMal, type: $type) { ...mediaCard } }";

pub struct AniList {
    client: reqwest::Client,
    endpoint: String,
}

impl AniList {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, endpoint: ENDPOINT.to_owned() }
    }

    /// Point the provider at another GraphQL endpoint (tests).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    async fn post(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": format!("{query} {CARD_FRAGMENT}"), "variables": variables }))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        let body: Value = resp.json().await.map_err(|e| ProviderError::Parse(e.without_url().to_string()))?;
        // GraphQL also reports rate limiting inside the body.
        if body["errors"].as_array().is_some_and(|e| e.iter().any(|x| x["status"] == 429)) {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        if !(200..300).contains(&status) {
            // AniList answers 404 for "Media not found" with an `errors` body: that is an empty result.
            if status == 404 {
                return Ok(body);
            }
            return Err(ProviderError::Http(status));
        }
        Ok(body)
    }

    fn page_vars(kind: MediaKind, page: u32) -> serde_json::Map<String, Value> {
        let mut v = serde_json::Map::new();
        v.insert("type".into(), kind.as_str().into());
        v.insert("page".into(), page.max(1).into());
        v.insert("perPage".into(), 20.into());
        v.insert("isAdult".into(), false.into());
        if kind == MediaKind::Manga {
            v.insert("format_not_in".into(), json!(["NOVEL"]));
        }
        v
    }

    /// Variables for a feed; `now` only matters for `Seasonal`.
    pub fn feed_vars(kind: MediaKind, feed: FeedKind, page: u32, now: OffsetDateTime) -> Value {
        let mut v = Self::page_vars(kind, page);
        match feed {
            FeedKind::Trending => {
                v.insert("sort".into(), json!(["TRENDING_DESC"]));
            }
            FeedKind::Popular => {
                v.insert("sort".into(), json!(["POPULARITY_DESC"]));
            }
            FeedKind::TopRated => {
                v.insert("sort".into(), json!(["SCORE_DESC"]));
            }
            FeedKind::Airing => {
                v.insert("status_in".into(), json!(["RELEASING"]));
                v.insert("sort".into(), json!(["POPULARITY_DESC"]));
            }
            FeedKind::Upcoming => {
                v.insert("status_in".into(), json!(["NOT_YET_RELEASED"]));
                v.insert("sort".into(), json!(["POPULARITY_DESC"]));
            }
            FeedKind::RecentlyCompleted => {
                v.insert("status_in".into(), json!(["FINISHED"]));
                v.insert("sort".into(), json!(["END_DATE_DESC"]));
            }
            FeedKind::Seasonal => {
                let year = now.year();
                if kind == MediaKind::Anime {
                    let season = match u8::from(now.month()) {
                        1..=3 => "WINTER",
                        4..=6 => "SPRING",
                        7..=9 => "SUMMER",
                        _ => "FALL",
                    };
                    v.insert("season".into(), season.into());
                    v.insert("seasonYear".into(), year.into());
                } else {
                    v.insert("startDate_greater".into(), (year * 10000).into());
                    v.insert("startDate_lesser".into(), ((year + 1) * 10000).into());
                }
                v.insert("sort".into(), json!(["POPULARITY_DESC"]));
            }
        }
        Value::Object(v)
    }
}

#[async_trait]
impl MetadataProvider for AniList {
    fn id(&self) -> &'static str {
        "anilist"
    }

    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime, MediaKind::Manga]
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        let vars = Self::feed_vars(k, f, page, OffsetDateTime::now_utc());
        map_anilist_page(&self.post(PAGE_QUERY, vars).await?)
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let q = q.trim();
        let mut v = Self::page_vars(k, page);
        if !q.is_empty() {
            v.insert("search".into(), q.into());
        }
        v.insert("sort".into(), json!([if q.is_empty() { "POPULARITY_DESC" } else { "SEARCH_MATCH" }]));
        map_anilist_page(&self.post(PAGE_QUERY, Value::Object(v)).await?)
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        if r.anilist_id.is_none() && r.mal_id.is_none() {
            return Ok(None);
        }
        let body = self.post(MEDIA_QUERY, json!({ "id": r.anilist_id, "idMal": r.mal_id, "type": k.as_str() })).await?;
        let m = &body["data"]["Media"];
        Ok(if m.is_object() { Some(map_anilist_media(m)) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ts(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(secs).unwrap()
    }

    #[test]
    fn seasonal_uses_the_injected_clock() {
        let v = AniList::feed_vars(MediaKind::Anime, FeedKind::Seasonal, 1, ts(1_793_102_400));
        assert_eq!(v["season"], "FALL");
        assert_eq!(v["seasonYear"], 2026);
        assert_eq!(v["isAdult"], false);
        let m = AniList::feed_vars(MediaKind::Manga, FeedKind::Seasonal, 2, ts(1_769_904_000));
        assert_eq!(m["startDate_greater"], 20260000);
        assert_eq!(m["page"], 2);
        assert_eq!(m["format_not_in"], json!(["NOVEL"]));
    }
}
