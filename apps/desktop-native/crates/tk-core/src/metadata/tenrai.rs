//! Tenrai provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.tenrai`; classic
//! `lib/metadata/providers/tenrai.ts`, `lib/tenrai/client.ts`). Last-resort source for titles AniList / Kitsu do not
//! have. Alive on 2026-11-09 (API 1.0.30): base `https://api.tenrai.org/v1`, Jikan v4 schema (`{ data, pagination }`),
//! keyed by **MAL id** (AniList `idMal`); spec at `https://api.tenrai.org/documentation/openapi.json` (the old prose
//! docs are stale). Public, no key: 120 req/min, 4 req/s per IP; `429` + `Retry-After` -> router cool-down; never the
//! private `X-Server-Key`. Routes b ports: `/anime/{id}/full`, `/anime/{id}/episodes?page=`, `/anime/{id}/characters`,
//! `/schedules`. Only if it stops answering during the wave does b mark it `discontinued`, remove it from
//! `with_defaults` and note it in docs/redesign/16 (plan 14 section 0.1 #13: "discarded with a note").

use async_trait::async_trait;

use super::MetadataProvider;
use crate::model::MediaKind;

pub struct Tenrai {
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl Tenrai {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

/// Contract stub: every call answers `Unsupported` (the router skips it without a cool-down) until the owner lands.
#[async_trait]
impl MetadataProvider for Tenrai {
    fn id(&self) -> &'static str {
        "tenrai"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime]
    }
}
