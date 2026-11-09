//! AniList / MyAnimeList lists (wave A, worker **g**; docs/redesign/15 section 4): OAuth through the system
//! browser and a loopback redirect ([`oauth`]), tokens in `crate::secrets`, read / write through
//! `metadata::anilist::lists` and [`mal`] (REST v2), local mirror + offline queue + conflicts in
//! `store::lists`. Contract: the token type, the service enum and the client entry points are final; bodies
//! are stubs. Worker g may add private files here (`sync.rs`, `import.rs`, `mal.rs` bodies ...).

pub mod mal;
pub mod oauth;

use serde::{Deserialize, Serialize};

use crate::error::ListError;

/// The two services. `id()` matches the UI `ListServiceVm::id()` and the `service` column of the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    AniList,
    Mal,
}

impl Service {
    pub const ALL: [Service; 2] = [Self::AniList, Self::Mal];

    pub fn id(self) -> &'static str {
        match self {
            Self::AniList => "anilist",
            Self::Mal => "mal",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// `crate::secrets::keys` entry of this service's token.
    pub fn secret_key(self) -> &'static str {
        match self {
            Self::AniList => crate::secrets::keys::ANILIST_TOKEN,
            Self::Mal => crate::secrets::keys::MAL_TOKEN,
        }
    }
}

/// A stored OAuth token (JSON in `Secrets`). AniList tokens last a year and have no refresh token; MAL tokens
/// last a month and refresh with `refresh_token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Token {
    pub service: Service,
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds; `None` = unknown / never.
    pub expires_at: Option<i64>,
    pub user_id: Option<u64>,
    pub username: Option<String>,
    pub avatar: Option<String>,
}

impl std::fmt::Display for Token {
    /// Never prints the tokens.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} token for {:?}", self.service.id(), self.username)
    }
}

/// Status names shared by the UI, the store and both services.
pub const STATUSES: [&str; 6] = ["watching", "planning", "completed", "paused", "dropped", "repeating"];

/// AniList `MediaListStatus` <-> shared status id.
pub fn anilist_status_to_shared(s: &str) -> Option<&'static str> {
    Some(match s {
        "CURRENT" => "watching",
        "PLANNING" => "planning",
        "COMPLETED" => "completed",
        "PAUSED" => "paused",
        "DROPPED" => "dropped",
        "REPEATING" => "repeating",
        _ => return None,
    })
}

pub fn shared_status_to_anilist(s: &str) -> Option<&'static str> {
    Some(match s {
        "watching" => "CURRENT",
        "planning" => "PLANNING",
        "completed" => "COMPLETED",
        "paused" => "PAUSED",
        "dropped" => "DROPPED",
        "repeating" => "REPEATING",
        _ => return None,
    })
}

/// MAL `status` <-> shared status id (MAL has no "repeating": `is_rewatching` on `watching`).
pub fn mal_status_to_shared(s: &str, rewatching: bool) -> Option<&'static str> {
    Some(match s {
        "watching" if rewatching => "repeating",
        "watching" => "watching",
        "plan_to_watch" => "planning",
        "completed" => "completed",
        "on_hold" => "paused",
        "dropped" => "dropped",
        _ => return None,
    })
}

pub fn shared_status_to_mal(s: &str) -> Option<(&'static str, bool)> {
    Some(match s {
        "watching" => ("watching", false),
        "repeating" => ("watching", true),
        "planning" => ("plan_to_watch", false),
        "completed" => ("completed", false),
        "paused" => ("on_hold", false),
        "dropped" => ("dropped", false),
        _ => return None,
    })
}

/// Refresh a MAL token when it is about to expire (AniList tokens are never refreshed). Contract stub.
pub async fn refresh_if_needed(client: &reqwest::Client, token: &Token, now: i64) -> Result<Option<Token>, ListError> {
    let _ = (client, token, now);
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mappings_round_trip() {
        for s in STATUSES {
            assert_eq!(anilist_status_to_shared(shared_status_to_anilist(s).unwrap()), Some(s));
            let (m, rw) = shared_status_to_mal(s).unwrap();
            assert_eq!(mal_status_to_shared(m, rw), Some(s));
        }
        assert_eq!(anilist_status_to_shared("NOPE"), None);
        assert_eq!(Service::from_id("mal"), Some(Service::Mal));
    }
}
