//! MangaDex provider (wave B, worker **l**; checklist `manga.metadata.mangadex`; classic `core/content` MangaDex
//! client). `https://api.mangadex.org`: `manga?title=` search, `manga/{id}` with `includes[]=cover_art,author`,
//! `manga/{id}/feed` for chapter lists (language, group, pages) used by `crate::manga` as one more chapter
//! provider; mapping to `MediaRef` through the `links.al` / `links.mal` attributes (AniList / MAL ids). Manga only.

use async_trait::async_trait;

use super::MetadataProvider;
use crate::model::MediaKind;

pub struct MangaDex {
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl MangaDex {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// Contract stub: every call answers `Unsupported` (the router skips it without a cool-down) until the owner lands.
#[async_trait]
impl MetadataProvider for MangaDex {
    fn id(&self) -> &'static str {
        "mangadex"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Manga]
    }
}
