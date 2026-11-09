//! OAuth helpers for the list services (wave A, worker **g**; docs/redesign/15 section 4). No crate: PKCE is
//! SHA-256 + base64url (both in the workspace) and the redirect listener is a tokio `TcpListener` on
//! `127.0.0.1:<port>` answering one plain HTTP request. The browser is opened by tk-app (`open` crate).
//!
//! AniList: authorization code grant, `https://anilist.co/api/v2/oauth/authorize?client_id=&redirect_uri=&
//! response_type=code`, token at `https://anilist.co/api/v2/oauth/token` (needs the client secret: use the
//! implicit grant `response_type=token` instead, the token arrives in the URL fragment and the loopback page
//! forwards it with a tiny script). MAL: authorization code + PKCE **plain** (MAL only supports `plain`:
//! `code_challenge = code_verifier`), `https://myanimelist.net/v1/oauth2/authorize`, token at
//! `https://myanimelist.net/v1/oauth2/token`. Client ids are build-time `option_env!` values read by tk-app
//! (`TATAKAI_ANILIST_CLIENT_ID`, `TATAKAI_MAL_CLIENT_ID`), never committed.
//!
//! Contract: [`pkce_verifier`] / [`pkce_challenge_s256`] are implemented (pure, tested); the listener and the
//! token exchange are stubs.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use crate::error::ListError;

/// Loopback redirect: `http://127.0.0.1:<port>/callback` (the port is picked from this range at connect time).
pub const LOOPBACK_PORTS: std::ops::RangeInclusive<u16> = 47_410..=47_430;
pub const CALLBACK_PATH: &str = "/callback";
/// How long the browser flow may take before `list_connect` fails with `ListError::OAuth("timeout")`.
pub const TIMEOUT_SECS: u64 = 300;

/// 64 random URL-safe characters (RFC 7636: 43..=128).
pub fn pkce_verifier() -> String {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(a.as_bytes());
    bytes.extend_from_slice(b.as_bytes());
    // 32 random bytes -> 43 base64url chars; pad with a second encoding slice to reach 64.
    let mut s = URL_SAFE_NO_PAD.encode(&bytes);
    let c = uuid::Uuid::new_v4();
    s.push_str(&URL_SAFE_NO_PAD.encode(c.as_bytes()));
    s.truncate(64);
    s
}

/// `BASE64URL(SHA256(verifier))` for `code_challenge_method=S256`.
pub fn pkce_challenge_s256(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// What the loopback listener received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    /// `code` (code grant) or `access_token` (implicit grant forwarded by the page).
    pub code_or_token: String,
    pub state: Option<String>,
}

/// Bind the first free `LOOPBACK_PORTS` port and wait for one `GET /callback?...` (or the fragment forwarded by
/// the page), answering a small "you can close this tab" HTML page. Cancel-safe; fails after `TIMEOUT_SECS`.
/// Returns the redirect URI that was used and the callback. Contract stub (worker g).
pub async fn wait_for_callback(expected_state: &str) -> Result<(String, Callback), ListError> {
    let _ = expected_state;
    Err(ListError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_values_have_the_rfc_shape() {
        let v = pkce_verifier();
        assert_eq!(v.len(), 64);
        assert!(v.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_ne!(v, pkce_verifier());
        // RFC 7636 appendix B
        assert_eq!(pkce_challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }
}
