//! Tokens, pairing codes and the viewer password (worker **k**; docs/redesign/13 D4 / D4b; classic
//! `server/src/security.mjs`). Bearer tokens are random 32-byte values (`uuid::Uuid::new_v4` twice, hex) stored
//! **hashed** (sha256) in `state.json`; pairing codes are 6 digits, 300 s, single use, at most 50 alive; the viewer
//! password is an argon2id PHC string (`argon2` crate, pure Rust; [`hash_password`] / [`verify_password`]).

use time::OffsetDateTime;

/// Lifetime of a pairing code (docs/13: `expiresIn: 300`).
pub const PAIRING_TTL_SECS: u32 = 300;
/// Live codes before `409 pairing_limit`.
pub const PAIRING_LIMIT: usize = 50;

/// `POST /auth/pairing` answer (`url == deep_link`, docs/13 D4).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub code: String,
    pub expires_in: u32,
    pub url: String,
    pub deep_link: String,
}

/// The canonical pairing link (docs/13 section 3.7): `tatakai://extensionserver/connect?server=<origin>&code=<code>`.
pub fn deep_link(origin: &str, code: &str) -> String {
    format!("tatakai://extensionserver/connect?server={}&code={}", url_encode(origin), url_encode(code))
}

/// Minimal percent-encoding of a query value (RFC 3986 unreserved kept).
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A six-digit code from random bytes (uniform enough for a 5-minute single-use secret).
pub fn new_code() -> String {
    let n = u32::from_le_bytes(uuid::Uuid::new_v4().as_bytes()[..4].try_into().expect("4 bytes")) % 1_000_000;
    format!("{n:06}")
}

/// A fresh bearer token (64 hex chars).
pub fn new_token() -> String {
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    format!("{}{}", a.simple(), b.simple())
}

/// sha256 hex of a token (what `state.json` stores).
pub fn token_hash(token: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

pub fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Viewer passwords: same bounds as the classic server (`security.mjs::passwordHash`, 10..=1024 characters).
pub const PASSWORD_MIN_CHARS: usize = 10;
pub const PASSWORD_MAX_CHARS: usize = 1024;

/// Whether `password` is within the accepted length.
pub fn password_len_ok(password: &str) -> bool {
    (PASSWORD_MIN_CHARS..=PASSWORD_MAX_CHARS).contains(&password.chars().count())
}

/// argon2id (v19, default params) PHC string of `password`, salted with 16 random bytes.
pub fn hash_password(password: &str) -> Result<String, String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt_bytes = uuid::Uuid::new_v4().into_bytes();
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| e.to_string())?;
    argon2::Argon2::default().hash_password(password.as_bytes(), &salt).map(|h| h.to_string()).map_err(|e| e.to_string())
}

/// Check `password` against a PHC string written by [`hash_password`]; any parse error is a mismatch.
pub fn verify_password(password: &str, phc: &str) -> bool {
    use argon2::password_hash::{PasswordHash, PasswordVerifier};
    if !password_len_ok(password) {
        return false;
    }
    let Ok(parsed) = PasswordHash::new(phc) else { return false };
    argon2::Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

/// Constant-time equality of two secrets (compares their sha256 digests byte by byte).
pub fn secrets_equal(a: &str, b: &str) -> bool {
    use sha2::Digest;
    let (x, y) = (sha2::Sha256::digest(a.as_bytes()), sha2::Sha256::digest(b.as_bytes()));
    x.iter().zip(y.iter()).fold(0_u8, |acc, (l, r)| acc | (l ^ r)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_link_is_canonical() {
        assert_eq!(
            deep_link("http://192.168.1.10:8099", "123456"),
            "tatakai://extensionserver/connect?server=http%3A%2F%2F192.168.1.10%3A8099&code=123456"
        );
    }

    #[test]
    fn codes_and_tokens_have_the_right_shape() {
        let c = new_code();
        assert_eq!(c.len(), 6);
        assert!(c.chars().all(|ch| ch.is_ascii_digit()));
        assert_eq!(new_token().len(), 64);
        assert_eq!(token_hash("a").len(), 64);
    }

    #[test]
    fn argon2id_round_trip() {
        let phc = hash_password("correct horse battery").unwrap();
        assert!(phc.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery", &phc));
        assert!(!verify_password("wrong horse battery", &phc));
        assert!(!verify_password("short", &phc));
        assert!(!verify_password("correct horse battery", "plain-placeholder:abc"));
        assert!(secrets_equal("abc", "abc"));
        assert!(!secrets_equal("abc", "abd"));
        assert!(password_len_ok("0123456789") && !password_len_ok("012345678"));
    }
}
