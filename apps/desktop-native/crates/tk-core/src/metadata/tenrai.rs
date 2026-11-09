//! Tenrai provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.tenrai`; classic
//! `lib/metadata/providers/tenrai.ts`). Last-resort source for titles AniList / Kitsu do not have: b reads the exact
//! endpoint and payload from the classic provider file and ports only what still answers. The classic host is a
//! public third-party API (allowed by the host guard). If the API is gone, b marks it `discontinued`, removes it from
//! `with_defaults` and notes it in docs/redesign/16 (plan 14 section 0.1 #13: "discarded with a note").

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
