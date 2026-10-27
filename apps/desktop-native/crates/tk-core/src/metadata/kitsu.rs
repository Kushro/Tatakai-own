//! Kitsu JSON:API provider (anime only in M0). Verified quirks (see legacy `kitsu.ts`): `page[limit]` max is 20,
//! `sort=-endDate` with a status filter 500s, adult titles are dropped client-side.

use async_trait::async_trait;
use serde_json::Value;
use time::OffsetDateTime;

use super::MetadataProvider;
use super::mapping::{map_kitsu_episodes, map_kitsu_list, map_kitsu_media};
use crate::error::ProviderError;
use crate::model::{Episode, FeedKind, FeedPage, Media, MediaKind, MediaRef};

const BASE: &str = "https://kitsu.io/api/edge";
const PAGE_LIMIT: u32 = 20;
const MAX_EPISODE_PAGES: u32 = 15;

pub struct Kitsu {
    client: reqwest::Client,
    base: String,
}

impl Kitsu {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, base: BASE.to_owned() }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ProviderError> {
        let url = url::Url::parse_with_params(&format!("{}{path}", self.base), params)
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        let resp = self.client.get(url).header("Accept", "application/vnd.api+json").send().await?;
        match resp.status().as_u16() {
            404 => Ok(serde_json::json!({ "data": [] })),
            429 => Err(ProviderError::RateLimited { provider: "kitsu".into() }),
            s if !(200..300).contains(&s) => Err(ProviderError::Http(s)),
            _ => resp.json::<Value>().await.map_err(|e| ProviderError::Parse(e.without_url().to_string())),
        }
    }

    fn list_params(page: u32) -> Vec<(&'static str, String)> {
        vec![
            ("include", "mappings,genres".into()),
            ("fields[mappings]", "externalSite,externalId".into()),
            ("fields[genres]", "name".into()),
            ("page[limit]", PAGE_LIMIT.to_string()),
            ("page[offset]", (page.max(1) - 1).saturating_mul(PAGE_LIMIT).to_string()),
        ]
    }

    /// Query params for a feed. Kitsu has no AniList-style trending: user count is the closest signal.
    pub fn feed_params(feed: FeedKind, page: u32, now: OffsetDateTime) -> Vec<(&'static str, String)> {
        let mut p = Self::list_params(page);
        match feed {
            FeedKind::Trending | FeedKind::Popular => p.push(("sort", "-userCount".into())),
            FeedKind::TopRated => p.push(("sort", "-averageRating".into())),
            FeedKind::Airing => {
                p.push(("filter[status]", "current".into()));
                p.push(("sort", "-userCount".into()));
            }
            FeedKind::Upcoming => {
                p.push(("filter[status]", "upcoming".into()));
                p.push(("sort", "-userCount".into()));
            }
            FeedKind::RecentlyCompleted => {
                p.push(("filter[status]", "finished".into()));
                p.push(("sort", "-startDate".into()));
            }
            FeedKind::Seasonal => {
                let season = match u8::from(now.month()) {
                    1..=3 => "winter",
                    4..=6 => "spring",
                    7..=9 => "summer",
                    _ => "fall",
                };
                p.push(("filter[season]", season.into()));
                p.push(("filter[seasonYear]", now.year().to_string()));
                p.push(("sort", "-userCount".into()));
            }
        }
        p
    }

    fn page_of(items_has_next: (Vec<Media>, bool), page: u32) -> FeedPage {
        // Adult titles carry ageRating "R18"/"R" with nsfw=true; the mapper does not read them, so drop via attributes in `adult_free`.
        FeedPage { items: items_has_next.0, page: page.max(1), has_next: items_has_next.1 }
    }

    /// Drop resources flagged `nsfw` before mapping.
    fn adult_free(mut doc: Value) -> Value {
        if let Some(d) = doc["data"].as_array_mut() {
            d.retain(|r| r["attributes"]["nsfw"] != true);
        }
        doc
    }

    async fn kitsu_id_of(&self, r: &MediaRef) -> Result<Option<u32>, ProviderError> {
        if let Some(id) = r.kitsu_id {
            return Ok(Some(id));
        }
        for (site, id) in [("anilist/anime", r.anilist_id), ("myanimelist/anime", r.mal_id)] {
            let Some(id) = id else { continue };
            let doc = self
                .get(
                    "/mappings",
                    &[
                        ("filter[externalSite]", site.to_owned()),
                        ("filter[externalId]", id.to_string()),
                        ("include", "item".into()),
                        ("page[limit]", "1".into()),
                    ],
                )
                .await?;
            let found = doc["data"][0]["relationships"]["item"]["data"]["id"].as_str().and_then(|s| s.parse().ok());
            if found.is_some() {
                return Ok(found);
            }
        }
        Ok(None)
    }
}

#[async_trait]
impl MetadataProvider for Kitsu {
    fn id(&self) -> &'static str {
        "kitsu"
    }

    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime]
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        if k != MediaKind::Anime {
            return Err(ProviderError::Unsupported);
        }
        let doc = Self::adult_free(self.get("/anime", &Self::feed_params(f, page, OffsetDateTime::now_utc())).await?);
        Ok(Self::page_of(map_kitsu_list(&doc), page))
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        if k != MediaKind::Anime {
            return Err(ProviderError::Unsupported);
        }
        let mut p = Self::list_params(page);
        p.push(("filter[text]", q.trim().to_owned()));
        let doc = Self::adult_free(self.get("/anime", &p).await?);
        Ok(Self::page_of(map_kitsu_list(&doc), page))
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        if k != MediaKind::Anime {
            return Err(ProviderError::Unsupported);
        }
        let Some(id) = self.kitsu_id_of(r).await? else { return Ok(None) };
        let doc = self
            .get(
                &format!("/anime/{id}"),
                &[
                    ("include", "mappings,genres".into()),
                    ("fields[mappings]", "externalSite,externalId".into()),
                    ("fields[genres]", "name".into()),
                ],
            )
            .await?;
        // A single resource comes back as an object under `data`.
        Ok(doc["data"].is_object().then(|| map_kitsu_media(&doc, &doc["data"])))
    }

    async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        let Some(id) = self.kitsu_id_of(r).await? else { return Ok(Vec::new()) };
        let mut out = Vec::new();
        for page in 0..MAX_EPISODE_PAGES {
            let doc = self
                .get(
                    &format!("/anime/{id}/episodes"),
                    &[
                        ("page[limit]", PAGE_LIMIT.to_string()),
                        ("page[offset]", (page * PAGE_LIMIT).to_string()),
                        ("sort", "number".into()),
                    ],
                )
                .await?;
            out.extend(map_kitsu_episodes(&doc));
            if doc["links"]["next"].as_str().is_none_or(str::is_empty) {
                break;
            }
        }
        out.sort_by_key(|e| e.number);
        Ok(out)
    }
}
