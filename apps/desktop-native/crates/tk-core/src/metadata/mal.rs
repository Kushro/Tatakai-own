//! MyAnimeList v2 API provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.mal`;
//! classic `lib/metadata/providers/mal.ts`). `https://api.myanimelist.net/v2` with `X-MAL-CLIENT-ID` =
//! `option_env!("TATAKAI_MAL_CLIENT_ID")` (the same public client id worker g's OAuth uses; without it `media` answers
//! `Unsupported` and Settings > Metadata explains). Fills score, rank, popularity, synopsis and `mean` when AniList lacks
//! them, keyed by `MediaRef::mal_id` (AniList gives `idMal`). `episodes` through `/anime/{id}/episodes`? MAL has none:
//! `Unsupported`. If the API no longer answers at all, b marks it `discontinued` in `ProviderHealthVm` and drops it
//! from `with_defaults` with a note in the contract (plan rule).

use async_trait::async_trait;

use super::MetadataProvider;
use crate::model::MediaKind;

pub struct Mal {
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl Mal {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// Contract stub: every call answers `Unsupported` (the router skips it without a cool-down) until the owner lands.
#[async_trait]
impl MetadataProvider for Mal {
    fn id(&self) -> &'static str {
        "mal"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime, MediaKind::Manga]
    }
}
