//! Shikimori provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.shikimori`;
//! classic `lib/metadata/providers/shikimori.ts`). `https://shikimori.one/api/animes/{mal id}` (`myanimelist_id` ==
//! MAL id on Shikimori) and `/animes/{id}/screenshots`: alternative titles (`russian`, `english`, `japanese`,
//! `synonyms`), score, screenshots; polite `User-Agent` required (`TatakaiNext/0.1`), 5 req/s. Keyed by
//! `MediaRef::mal_id`. If the API no longer answers, b drops it with a note (plan rule).

use async_trait::async_trait;

use super::MetadataProvider;
use crate::model::MediaKind;

pub struct Shikimori {
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl Shikimori {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// Contract stub: every call answers `Unsupported` (the router skips it without a cool-down) until the owner lands.
#[async_trait]
impl MetadataProvider for Shikimori {
    fn id(&self) -> &'static str {
        "shikimori"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime, MediaKind::Manga]
    }
}
