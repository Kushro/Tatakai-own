//! ani.zip: episode lists, id mappings and series artwork (anime only, not a catalogue).
//!
//! 0.3.2: answers are kept in a process-wide cache ([`RAW_TTL`], at most [`RAW_MAX`] titles, 404s included) shared by
//! every `AniZip` value, so the episode merge, the spotlight artwork and Details ask ani.zip once per title.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use super::mapping::{map_anizip_episodes, map_anizip_ids, map_anizip_images};
use crate::error::ProviderError;
use crate::model::{Episode, MediaKind, MediaRef, SeriesArt};

const BASE: &str = "https://api.ani.zip/mappings";
/// How long one ani.zip answer is reused.
pub const RAW_TTL: Duration = Duration::from_secs(30 * 60);
/// Titles kept in the raw cache (the oldest goes first).
const RAW_MAX: usize = 96;

type RawCache = HashMap<String, (Instant, Option<Value>)>;
static RAW: Mutex<Option<RawCache>> = Mutex::new(None);

fn cached(url: &str) -> Option<Option<Value>> {
    let g = RAW.lock().ok()?;
    let (at, v) = g.as_ref()?.get(url)?;
    (at.elapsed() < RAW_TTL).then(|| v.clone())
}

fn remember(url: String, v: Option<Value>) {
    let Ok(mut g) = RAW.lock() else { return };
    let m = g.get_or_insert_with(HashMap::new);
    m.retain(|_, (at, _)| at.elapsed() < RAW_TTL);
    while m.len() >= RAW_MAX {
        let Some(oldest) = m.iter().min_by_key(|(_, (at, _))| *at).map(|(k, _)| k.clone()) else { break };
        m.remove(&oldest);
    }
    m.insert(url, (Instant::now(), v));
}

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
        let url = format!("{}?{q}", self.base);
        if let Some(hit) = cached(&url) {
            return Ok(hit);
        }
        let resp = self.client.get(&url).send().await?;
        let out = match resp.status().as_u16() {
            404 => Ok(None),
            429 => Err(ProviderError::RateLimited { provider: "anizip".into() }),
            s if !(200..300).contains(&s) => Err(ProviderError::Http(s)),
            _ => resp
                .json::<Value>()
                .await
                .map(Some)
                .map_err(|e| ProviderError::Parse(e.without_url().to_string())),
        };
        if let Ok(v) = &out {
            remember(url, v.clone());
        }
        out
    }

    /// Series artwork (fanart, banner, poster, clear logo); `None` when ani.zip does not know the title.
    pub async fn art(&self, r: &MediaRef) -> Result<Option<SeriesArt>, ProviderError> {
        Ok(self.raw(r).await?.map(|raw| map_anizip_images(&raw)))
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
