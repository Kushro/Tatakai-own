//! MangaUpdates provider (wave B, worker **l**; checklist `manga.metadata.mangaupdates`; classic `core/content`
//! MangaUpdates client). `https://api.mangaupdates.com/v1`: `series/search` (POST), `series/{id}`: publisher,
//! serialisation, publication status, cross-site rating. Manga only; `media` enriches the AniList row (never replaces
//! it) and `feed` / `search` answer `Unsupported`.

use async_trait::async_trait;

use super::MetadataProvider;
use crate::model::MediaKind;

pub struct MangaUpdates {
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl MangaUpdates {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// Contract stub: every call answers `Unsupported` (the router skips it without a cool-down) until the owner lands.
#[async_trait]
impl MetadataProvider for MangaUpdates {
    fn id(&self) -> &'static str {
        "mangaupdates"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Manga]
    }
}
