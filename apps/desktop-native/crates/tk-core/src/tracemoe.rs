//! trace.moe scene search (wave A, worker **a**; docs/redesign/15 section 4). `GET https://api.trace.moe/search?url=`
//! or `POST https://api.trace.moe/search` with the raw image body (`Content-Type: image/*`); add `anilistInfo` to
//! get titles, or map `anilist` ids through the metadata router. Contract: types final, body is a stub.

use crate::error::ProviderError;

pub const ENDPOINT: &str = "https://api.trace.moe/search";

/// What to search with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageQuery {
    Url(String),
    Bytes { data: Vec<u8>, mime: String },
}

/// One result (`result[]` of the response).
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub anilist_id: u32,
    pub episode: Option<u32>,
    pub from_secs: f64,
    pub to_secs: f64,
    /// 0.0..=1.0
    pub similarity: f32,
    pub image: Option<String>,
    pub video: Option<String>,
}

pub struct TraceMoe {
    client: reqwest::Client,
    endpoint: String,
}

impl TraceMoe {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, endpoint: ENDPOINT.to_owned() }
    }

    /// Tests.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Best match first. Contract stub (worker a).
    pub async fn search(&self, q: &ImageQuery, cut_borders: bool) -> Result<Vec<Match>, ProviderError> {
        let _ = (q, cut_borders, &self.client, &self.endpoint);
        Err(ProviderError::Unsupported)
    }
}
