//! AniList GraphQL provider (anime + manga catalogue; no episode list).
//!
//! Wave A split (docs/redesign/15-parity-wave-a-contract.md section 2): this file keeps the client, `post`,
//! the feed / search / media queries and the `MetadataProvider` impl (frozen); [`search`] (worker a) adds the
//! filtered search, studios and characters; [`details`] (worker c) the relations / characters / staff /
//! recommendations / facts query; [`lists`] (worker g) the authenticated viewer / `MediaList` queries and
//! mutations. The three are child modules: they call the private `post` of this file.

/// Wave B, worker b: airing schedule, genres, top ranges, upcoming / most-favourite feeds.
pub mod airing;
/// 0.3.1: several queries in one request (GraphQL aliases) and the split of the answer.
pub mod batch;
pub mod details;
/// 0.3.1: the one shared AniList door (limiter, 429 policy, priorities, de-duplication, diagnostics).
pub mod gateway;
pub mod lists;
/// Wave B, worker l (m reads): `MANGA` type search / details / list, `NOVEL` format.
pub mod manga;
pub mod search;
/// Wave B, worker n: profile, follows, activity, notifications, reviews.
pub mod social;

use std::sync::Arc;
use std::time::Duration;

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

/// 0.3.1: the card without `description` for rows that never show it (every row but the Home spotlight; Details
/// asks [`MEDIA_QUERY`] with the full card). Same fragment name, so the queries need no change.
pub(crate) const CARD_LITE_FRAGMENT: &str = "fragment mediaCard on Media { id idMal type format status season seasonYear \
episodes title { romaji english native } coverImage { extraLarge large medium color } bannerImage averageScore genres }";

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
    /// 0.3.1: every request goes through this (the process-wide [`gateway::global`] in production).
    gateway: Arc<gateway::Gateway>,
}

impl AniList {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, endpoint: ENDPOINT.to_owned(), gateway: gateway::global() }
    }

    /// Point the provider at another GraphQL endpoint (tests). It gets a gateway of its own, so test servers do not
    /// share (or spend) the budget of the real AniList.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self.gateway = Arc::new(gateway::Gateway::new());
        self
    }

    /// Use this gateway (tests share one between several clients).
    pub fn with_gateway(mut self, g: Arc<gateway::Gateway>) -> Self {
        self.gateway = g;
        self
    }

    pub fn gateway(&self) -> &Arc<gateway::Gateway> {
        &self.gateway
    }

    /// One GraphQL request through the gateway, answered as status + body.
    pub(crate) async fn send(
        &self,
        query: &str,
        variables: Value,
        token: Option<&str>,
    ) -> Result<gateway::Reply, gateway::GatewayError> {
        self.gateway.execute(&self.client, &self.endpoint, query, variables, token).await
    }

    /// POST a query that uses `...mediaCard` (the fragment is appended).
    async fn post(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        self.post_bare(&format!("{query} {CARD_FRAGMENT}"), variables).await
    }

    /// POST a query that uses `...mediaCard` with the light card (no `description`).
    async fn post_lite(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        self.post_bare(&format!("{query} {CARD_LITE_FRAGMENT}"), variables).await
    }

    /// POST a query as is (no fragment appended); maps status and GraphQL errors to [`ProviderError`].
    pub(crate) async fn post_bare(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        let r = self.send(query, variables, None).await?;
        provider_reply(r)
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

/// Status -> result: 2xx and 404 (AniList answers "Media not found" with an `errors` body: an empty result) are
/// bodies, anything else [`ProviderError::Http`]. Rate limiting never gets here (the gateway answers it).
fn provider_reply(r: gateway::Reply) -> Result<Value, ProviderError> {
    if !(200..300).contains(&r.status) && r.status != 404 {
        return Err(ProviderError::Http(r.status));
    }
    Ok(r.body)
}

#[async_trait]
impl MetadataProvider for AniList {
    fn id(&self) -> &'static str {
        "anilist"
    }

    fn cooldown_for(&self, err: &ProviderError) -> Option<Duration> {
        match err {
            // Exactly as long as AniList asked (Retry-After / Reset), not the fixed router cool-down.
            ProviderError::RateLimited { .. } => Some(self.gateway.blocked_for()).filter(|d| !d.is_zero()),
            _ => None,
        }
    }

    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime, MediaKind::Manga]
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        let vars = Self::feed_vars(k, f, page, OffsetDateTime::now_utc());
        // Trending feeds the Home spotlight, which shows the synopsis; the other rows do not.
        let body = if f == FeedKind::Trending { self.post(PAGE_QUERY, vars).await? } else { self.post_lite(PAGE_QUERY, vars).await? };
        map_anilist_page(&body)
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let q = q.trim();
        let mut v = Self::page_vars(k, page);
        if !q.is_empty() {
            v.insert("search".into(), q.into());
        }
        v.insert("sort".into(), json!([if q.is_empty() { "POPULARITY_DESC" } else { "SEARCH_MATCH" }]));
        map_anilist_page(&self.post_lite(PAGE_QUERY, Value::Object(v)).await?)
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        if r.anilist_id.is_none() && r.mal_id.is_none() {
            return Ok(None);
        }
        let mut vars = details::id_vars(r);
        vars["type"] = json!(k.as_str());
        let body = self.post(MEDIA_QUERY, vars).await?;
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
