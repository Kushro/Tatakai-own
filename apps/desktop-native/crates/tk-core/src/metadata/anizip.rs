//! ani.zip: episode lists and id mappings (anime only, not a catalogue).

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use super::mapping::{map_anizip_episodes, map_anizip_ids};
use crate::error::ProviderError;
use crate::model::{Episode, MediaKind, MediaRef};

const BASE: &str = "https://api.ani.zip/mappings";

pub struct AniZip {
    client: reqwest::Client,
    base: String,
}

impl AniZip {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, base: BASE.to_owned() }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    fn query_for(r: &MediaRef) -> Option<String> {
        if let Some(id) = r.anilist_id.filter(|i| *i > 0) {
            Some(format!("anilist_id={id}"))
        } else if let Some(id) = r.mal_id.filter(|i| *i > 0) {
            Some(format!("mal_id={id}"))
        } else {
            r.kitsu_id.filter(|i| *i > 0).map(|id| format!("kitsu_id={id}"))
        }
    }

    async fn raw(&self, r: &MediaRef) -> Result<Option<Value>, ProviderError> {
        let Some(q) = Self::query_for(r) else { return Ok(None) };
        let resp = self.client.get(format!("{}?{q}", self.base)).send().await?;
        match resp.status().as_u16() {
            404 => Ok(None),
            429 => Err(ProviderError::RateLimited { provider: "anizip".into() }),
            s if !(200..300).contains(&s) => Err(ProviderError::Http(s)),
            _ => resp
                .json::<Value>()
                .await
                .map(Some)
                .map_err(|e| ProviderError::Parse(e.without_url().to_string())),
        }
    }

    /// Cross-provider id mapping (AniList / MAL / Kitsu) for a reference with at least one id.
    pub async fn ids(&self, r: &MediaRef) -> Result<Option<MediaRef>, ProviderError> {
        Ok(self.raw(r).await?.map(|raw| map_anizip_ids(&raw)).filter(|m| m.anilist_id.is_some()))
    }
}

#[async_trait]
impl MetadataProvider for AniZip {
    fn id(&self) -> &'static str {
        "anizip"
    }

    fn kinds(&self) -> &[MediaKind] {
        &[]
    }

    async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        Ok(self.raw(r).await?.map(|raw| map_anizip_episodes(&raw)).unwrap_or_default())
    }
}
