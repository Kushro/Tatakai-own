//! Debrid services as torrent sources (wave A, worker **i**; docs/redesign/15 section 4): Real-Debrid
//! (`api.real-debrid.com/rest/1.0`), TorBox (`api.torbox.app/v1/api`), AllDebrid (`api.alldebrid.com/v4`) and
//! Premiumize (`www.premiumize.me/api`). Flow: `check_cached(info_hashes)` marks picker rows; `resolve(magnet,
//! episode file)` adds the magnet to the account, selects the file and returns a direct HTTPS link the player
//! loads as a web stream. Keys live in `crate::secrets` (`keys::debrid(service.id())`). Contract: [`Service`],
//! [`Account`], [`Resolved`] and the [`DebridClient`] entry points are final; bodies are stubs. Worker i adds one
//! private file per service here.

use crate::error::DebridError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl Service {
    pub const ALL: [Service; 4] = [Self::RealDebrid, Self::TorBox, Self::AllDebrid, Self::Premiumize];

    /// Matches the UI `DebridServiceVm::id()` and the `service` column of `debrid_cache`.
    pub fn id(self) -> &'static str {
        match self {
            Self::RealDebrid => "real_debrid",
            Self::TorBox => "torbox",
            Self::AllDebrid => "alldebrid",
            Self::Premiumize => "premiumize",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub service: Service,
    pub username: Option<String>,
    /// Unix seconds, when the service says.
    pub premium_until: Option<i64>,
}

/// A direct link for one file of a cached torrent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub url: String,
    pub file_name: String,
    pub size_bytes: Option<u64>,
}

pub struct DebridClient {
    client: reqwest::Client,
}

impl DebridClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Validate `key` and return the account. Contract stub (worker i).
    pub async fn account(&self, service: Service, key: &str) -> Result<Account, DebridError> {
        let _ = (service, key, &self.client);
        Err(DebridError::Unsupported)
    }

    /// Which of `info_hashes` (lowercase hex) the service has cached. Contract stub (worker i).
    pub async fn check_cached(&self, service: Service, key: &str, info_hashes: &[String]) -> Result<Vec<(String, bool)>, DebridError> {
        let _ = (service, key, info_hashes);
        Err(DebridError::Unsupported)
    }

    /// Add `magnet`, pick the file for `episode` (`tk_core::release::pick_episode_file` over the file list) and
    /// return its direct link. Contract stub (worker i).
    pub async fn resolve(&self, service: Service, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
        let _ = (service, key, magnet, episode, titles);
        Err(DebridError::Unsupported)
    }
}
