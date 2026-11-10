//! AniList / MyAnimeList lists (wave A, worker **g**; docs/redesign/15 section 4): OAuth through the system
//! browser and a loopback redirect ([`oauth`]), tokens in `crate::secrets`, read / write through
//! `metadata::anilist::lists` and [`mal`] (REST v2), local mirror + offline queue + conflicts in
//! `store::lists`. Contract: the token type, the service enum and the client entry points are final.
//! [`sync`] holds the pure three-way merge rules (local / last-synced base / remote) and the automatic progress
//! rule; [`import`] the MAL XML / AniList JSON / Tatakai JSON parsers and the JSON / CSV writers.

pub mod import;
pub mod mal;
pub mod oauth;
pub mod progress;
pub mod sync;

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
pub const STATUSES: [&str; 6] = [
    "watching",
    "planning",
    "completed",
    "paused",
    "dropped",
    "repeating",
];

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

/// Environment variable (runtime first, then the value baked in at build time) with the OAuth client id.
pub fn client_id_var(service: Service) -> &'static str {
    match service {
        Service::AniList => "TATAKAI_ANILIST_CLIENT_ID",
        Service::Mal => "TATAKAI_MAL_CLIENT_ID",
    }
}

/// The OAuth client id of `service`: the runtime environment variable when set (developer builds), else the
/// build-time `option_env!` value. `None` = "Connect" explains that the build has no client id.
pub fn client_id(service: Service) -> Option<String> {
    let baked = match service {
        Service::AniList => option_env!("TATAKAI_ANILIST_CLIENT_ID"),
        Service::Mal => option_env!("TATAKAI_MAL_CLIENT_ID"),
    };
    std::env::var(client_id_var(service))
        .ok()
        .or_else(|| baked.map(str::to_owned))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// Refresh this long before the expiry (MAL tokens last 31 days).
pub const REFRESH_MARGIN_SECS: i64 = 24 * 3600;

/// Whether `token` must be refreshed now: only MAL tokens with a refresh token and a known expiry.
pub fn needs_refresh(token: &Token, now: i64) -> bool {
    token.service == Service::Mal
        && token.refresh_token.is_some()
        && token.expires_at.is_some_and(|at| at - now <= REFRESH_MARGIN_SECS)
}

/// Whether the token is past its expiry and cannot be refreshed (AniList after a year, MAL without refresh token).
pub fn is_expired(token: &Token, now: i64) -> bool {
    token.expires_at.is_some_and(|at| at <= now) && !needs_refresh(token, now)
}

/// Refresh a MAL token when it is about to expire (AniList tokens are never refreshed). `Ok(None)` = still
/// valid; `Ok(Some(new))` = refreshed (the caller stores it; user fields are carried over); a refused refresh
/// token is `ListError::Unauthorized`.
pub async fn refresh_if_needed(
    client: &reqwest::Client,
    token: &Token,
    now: i64,
) -> Result<Option<Token>, ListError> {
    if !needs_refresh(token, now) {
        return Ok(None);
    }
    let client_id = client_id(Service::Mal).ok_or_else(|| ListError::OAuth("no_client_id".into()))?;
    let refresh = token.refresh_token.as_deref().unwrap_or_default();
    let mut fresh = oauth::mal_refresh(client, &client_id, refresh, now).await?;
    fresh.user_id = token.user_id;
    fresh.username = token.username.clone();
    fresh.avatar = token.avatar.clone();
    if fresh.refresh_token.is_none() {
        fresh.refresh_token = token.refresh_token.clone();
    }
    Ok(Some(fresh))
}

/// Shared 0..=100 score -> MAL 0..=10 (rounded; `None` / 0 = unscored).
pub fn score_to_mal(score_100: Option<u8>) -> u8 {
    score_100.map_or(0, |s| ((u16::from(s.min(100)) + 5) / 10) as u8)
}

/// MAL 0..=10 -> shared 0..=100 (`0` = unscored).
pub fn score_from_mal(score: u8) -> Option<u8> {
    (score > 0).then(|| score.min(10) * 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mappings_round_trip() {
        for s in STATUSES {
            assert_eq!(
                anilist_status_to_shared(shared_status_to_anilist(s).unwrap()),
                Some(s)
            );
            let (m, rw) = shared_status_to_mal(s).unwrap();
            assert_eq!(mal_status_to_shared(m, rw), Some(s));
        }
        assert_eq!(anilist_status_to_shared("NOPE"), None);
        assert_eq!(Service::from_id("mal"), Some(Service::Mal));
    }

    fn token(service: Service, refresh: bool, expires_at: Option<i64>) -> Token {
        Token {
            service,
            access_token: "a".into(),
            refresh_token: refresh.then(|| "r".to_owned()),
            expires_at,
            user_id: None,
            username: Some("u".into()),
            avatar: None,
        }
    }

    #[test]
    fn refresh_rules() {
        let now = 1_000_000;
        assert!(needs_refresh(&token(Service::Mal, true, Some(now + 3600)), now));
        assert!(!needs_refresh(
            &token(Service::Mal, true, Some(now + 10 * 86_400)),
            now
        ));
        assert!(!needs_refresh(&token(Service::Mal, false, Some(now - 1)), now));
        assert!(!needs_refresh(&token(Service::AniList, true, Some(now)), now));
        assert!(is_expired(&token(Service::AniList, false, Some(now - 1)), now));
        assert!(
            !is_expired(&token(Service::Mal, true, Some(now - 1)), now),
            "refreshable, not expired"
        );
        assert!(!is_expired(&token(Service::AniList, false, None), now));
        let mut secret = token(Service::Mal, true, None);
        secret.access_token = "very-secret".into();
        assert!(
            !secret.to_string().contains("very-secret"),
            "Display never prints the token"
        );
    }

    #[tokio::test]
    async fn refresh_if_needed_is_a_no_op_for_valid_tokens() {
        let c = reqwest::Client::new();
        assert_eq!(
            refresh_if_needed(&c, &token(Service::AniList, false, Some(5)), 10)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            refresh_if_needed(&c, &token(Service::Mal, true, Some(10 * 86_400)), 0)
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn mal_scores() {
        assert_eq!(score_to_mal(None), 0);
        assert_eq!(score_to_mal(Some(85)), 9);
        assert_eq!(score_to_mal(Some(84)), 8);
        assert_eq!(score_to_mal(Some(100)), 10);
        assert_eq!(score_from_mal(0), None);
        assert_eq!(score_from_mal(7), Some(70));
    }
}
