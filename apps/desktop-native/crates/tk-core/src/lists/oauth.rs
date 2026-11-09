//! OAuth helpers for the list services (wave A, worker **g**; docs/redesign/15 section 4). No crate: PKCE is
//! SHA-256 + base64url (both in the workspace) and the redirect listener is a tokio `TcpListener` on
//! `127.0.0.1:<port>` answering plain HTTP requests. The browser is opened by tk-app (`open` crate).
//!
//! AniList: implicit grant (`response_type=token`, no client secret needed). The token arrives in the URL
//! fragment, which browsers never send to a server, so the first `GET /callback` (without a query) answers a tiny
//! page whose script forwards the fragment as a query string (`/callback?access_token=...&forwarded=1`). AniList
//! allows one redirect URL per client: register `http://127.0.0.1:47410/callback` (the first port of
//! [`LOOPBACK_PORTS`]); [`bind_loopback`] with `first_only` binds exactly that port.
//!
//! MAL: authorization code + PKCE **plain** (MAL only supports `plain`: `code_challenge = code_verifier`),
//! `https://myanimelist.net/v1/oauth2/authorize`, token at `https://myanimelist.net/v1/oauth2/token`
//! ([`mal_exchange_code`], [`mal_refresh`]). Client ids: `crate::lists::client_id` (runtime env var, else the
//! build-time `option_env!`), never committed.
//!
//! Errors are `ListError::OAuth(<code>)` with a fixed code set the UI translates: `no_client_id`, `port`,
//! `browser`, `timeout`, `denied`, `state`, `invalid_callback`, `exchange` (and `provider:<error>` for any other
//! `error=` the service sends back).

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{Service, Token};
use crate::error::ListError;

/// Loopback redirect: `http://127.0.0.1:<port>/callback` (the port is picked from this range at connect time).
pub const LOOPBACK_PORTS: std::ops::RangeInclusive<u16> = 47_410..=47_430;
pub const CALLBACK_PATH: &str = "/callback";
/// How long the browser flow may take before `list_connect` fails with `ListError::OAuth("timeout")`.
pub const TIMEOUT_SECS: u64 = 300;

pub const ANILIST_AUTHORIZE: &str = "https://anilist.co/api/v2/oauth/authorize";
pub const MAL_AUTHORIZE: &str = "https://myanimelist.net/v1/oauth2/authorize";
pub const MAL_TOKEN: &str = "https://myanimelist.net/v1/oauth2/token";
/// AniList implicit-grant tokens are valid for one year.
pub const ANILIST_TOKEN_SECS: i64 = 365 * 24 * 3600;

/// Largest request head the listener reads.
const MAX_REQUEST: usize = 16 * 1024;

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

/// Random `state` value (32 URL-safe characters).
pub fn new_state() -> String {
    let mut s = pkce_verifier();
    s.truncate(32);
    s
}

/// What the loopback listener received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Callback {
    /// `code` (code grant) or `access_token` (implicit grant forwarded by the page).
    pub code_or_token: String,
    pub state: Option<String>,
}

/// Browser URL of the AniList implicit grant (the redirect is the one registered for the client).
pub fn anilist_authorize_url(client_id: &str, state: &str) -> String {
    let params = [
        ("client_id", client_id),
        ("response_type", "token"),
        ("state", state),
    ];
    url::Url::parse_with_params(ANILIST_AUTHORIZE, params)
        .map(String::from)
        .unwrap_or_default()
}

/// Browser URL of the MAL authorization-code grant with PKCE `plain` (`code_challenge = verifier`).
pub fn mal_authorize_url(client_id: &str, verifier: &str, state: &str, redirect_uri: &str) -> String {
    let params = [
        ("response_type", "code"),
        ("client_id", client_id),
        ("code_challenge", verifier),
        ("code_challenge_method", "plain"),
        ("state", state),
        ("redirect_uri", redirect_uri),
    ];
    url::Url::parse_with_params(MAL_AUTHORIZE, params)
        .map(String::from)
        .unwrap_or_default()
}

/// A bound loopback listener and the redirect URI that reaches it.
pub struct Loopback {
    listener: TcpListener,
    pub redirect_uri: String,
}

/// Bind `127.0.0.1` on the first free port of [`LOOPBACK_PORTS`] (`first_only`: only the first one, for services
/// that accept a single registered redirect). Fails with `OAuth("port")` when none is free.
pub async fn bind_loopback(first_only: bool) -> Result<Loopback, ListError> {
    let ports: Vec<u16> = if first_only {
        vec![*LOOPBACK_PORTS.start()]
    } else {
        LOOPBACK_PORTS.collect()
    };
    for port in ports {
        if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            return Ok(Loopback {
                listener,
                redirect_uri: redirect_uri(port),
            });
        }
    }
    Err(ListError::OAuth("port".into()))
}

pub fn redirect_uri(port: u16) -> String {
    format!("http://127.0.0.1:{port}{CALLBACK_PATH}")
}

impl Loopback {
    pub fn port(&self) -> Option<u16> {
        self.listener.local_addr().ok().map(|a| a.port())
    }

    /// Serve requests until the callback arrives (or fails), at most [`TIMEOUT_SECS`]. Dropping the future
    /// closes the listener (cancel-safe).
    pub async fn accept(self, expected_state: &str) -> Result<Callback, ListError> {
        self.accept_within(expected_state, Duration::from_secs(TIMEOUT_SECS))
            .await
    }

    pub async fn accept_within(self, expected_state: &str, limit: Duration) -> Result<Callback, ListError> {
        let run = async {
            loop {
                let Ok((stream, _)) = self.listener.accept().await else {
                    continue;
                };
                if let Some(result) = serve_one(stream, expected_state).await {
                    return result;
                }
            }
        };
        match tokio::time::timeout(limit, run).await {
            Ok(r) => r,
            Err(_) => Err(ListError::OAuth("timeout".into())),
        }
    }
}

/// Bind the first free `LOOPBACK_PORTS` port and wait for one `GET /callback?...` (or the fragment forwarded by
/// the page), answering a small "you can close this tab" HTML page. Cancel-safe; fails after `TIMEOUT_SECS`.
/// Returns the redirect URI that was used and the callback. Callers that must put the redirect URI in the browser
/// URL first use [`bind_loopback`] + [`Loopback::accept`].
pub async fn wait_for_callback(expected_state: &str) -> Result<(String, Callback), ListError> {
    let lb = bind_loopback(false).await?;
    let uri = lb.redirect_uri.clone();
    let cb = lb.accept(expected_state).await?;
    Ok((uri, cb))
}

/// What one request to the listener means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Not the callback (favicon, other path): 404, keep waiting.
    Ignore,
    /// `GET /callback` without a query: the token is in the fragment; answer the forwarding page.
    Forward,
    Done(Callback),
    Failed(String),
}

/// Classify a request target (`/callback?code=...`). Pure: unit tested.
pub fn classify(target: &str, expected_state: &str) -> Outcome {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != CALLBACK_PATH {
        return Outcome::Ignore;
    }
    let params: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    if params.is_empty() {
        return Outcome::Forward;
    }
    if let Some(e) = params.get("error") {
        return Outcome::Failed(match e.as_str() {
            "access_denied" => "denied".to_owned(),
            "no_token" => "invalid_callback".to_owned(),
            other => format!("provider:{other}"),
        });
    }
    let state = params.get("state").cloned();
    if let Some(token) = params.get("access_token").filter(|t| !t.is_empty()) {
        // Implicit grant: AniList may not echo `state`; when it does, it must match.
        if state.as_deref().is_some_and(|s| s != expected_state) {
            return Outcome::Failed("state".into());
        }
        return Outcome::Done(Callback {
            code_or_token: token.clone(),
            state,
        });
    }
    if let Some(code) = params.get("code").filter(|c| !c.is_empty()) {
        if state.as_deref() != Some(expected_state) {
            return Outcome::Failed("state".into());
        }
        return Outcome::Done(Callback {
            code_or_token: code.clone(),
            state,
        });
    }
    Outcome::Failed("invalid_callback".into())
}

/// `GET /callback?x HTTP/1.1` -> `/callback?x`.
pub fn request_target(head: &str) -> Option<&str> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method == "GET").then_some(target)
}

const DONE_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Tatakai</title></head>\
<body style=\"font-family:sans-serif;margin:3rem\"><h1>Tatakai</h1><p>Account connected. You can close this tab \
and return to Tatakai.</p><p>Cuenta conectada. Ya puedes cerrar esta pesta&ntilde;a y volver a Tatakai.</p></body></html>";

const FAILED_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Tatakai</title></head>\
<body style=\"font-family:sans-serif;margin:3rem\"><h1>Tatakai</h1><p>The account was not connected. Return to \
Tatakai for details.</p><p>No se conect&oacute; la cuenta. Vuelve a Tatakai para ver el motivo.</p></body></html>";

/// Forwards `#access_token=...` as a query string to the same listener.
const FORWARD_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Tatakai</title></head>\
<body style=\"font-family:sans-serif;margin:3rem\"><p>Tatakai&hellip;</p><script>\
var h=location.hash.replace(/^#/,'');location.replace('/callback?'+(h||'error=no_token')+'&forwarded=1');\
</script></body></html>";

/// Read one request, answer it. `None` = keep waiting.
async fn serve_one(mut stream: TcpStream, expected_state: &str) -> Option<Result<Callback, ListError>> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0_u8; 2048];
    let read = async {
        loop {
            let n = stream.read(&mut chunk).await.ok()?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() >= MAX_REQUEST {
                break;
            }
        }
        Some(())
    };
    // A stalled connection (browser pre-connect) must not block the real callback forever.
    if tokio::time::timeout(Duration::from_secs(10), read)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return None;
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    let Some(target) = request_target(&head) else {
        let _ = respond(&mut stream, "400 Bad Request", "").await;
        return None;
    };
    match classify(target, expected_state) {
        Outcome::Ignore => {
            let _ = respond(&mut stream, "404 Not Found", "").await;
            None
        }
        Outcome::Forward => {
            let _ = respond(&mut stream, "200 OK", FORWARD_PAGE).await;
            None
        }
        Outcome::Done(cb) => {
            let _ = respond(&mut stream, "200 OK", DONE_PAGE).await;
            Some(Ok(cb))
        }
        Outcome::Failed(code) => {
            let _ = respond(&mut stream, "200 OK", FAILED_PAGE).await;
            Some(Err(ListError::OAuth(code)))
        }
    }
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await
}

/// MAL token endpoint answer -> [`Token`] (user fields filled later from `users/@me`).
pub fn token_from_mal_json(v: &Value, now: i64) -> Result<Token, ListError> {
    let access = v["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ListError::OAuth("exchange".into()))?;
    Ok(Token {
        service: Service::Mal,
        access_token: access.to_owned(),
        refresh_token: v["refresh_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
        expires_at: v["expires_in"].as_i64().map(|s| now + s),
        user_id: None,
        username: None,
        avatar: None,
    })
}

/// A fresh AniList token from the implicit grant.
pub fn anilist_token(access_token: &str, now: i64) -> Token {
    Token {
        service: Service::AniList,
        access_token: access_token.to_owned(),
        refresh_token: None,
        expires_at: Some(now + ANILIST_TOKEN_SECS),
        user_id: None,
        username: None,
        avatar: None,
    }
}

async fn mal_token_request(
    client: &reqwest::Client,
    endpoint: &str,
    form: &[(&str, &str)],
    now: i64,
) -> Result<Token, ListError> {
    let resp = client
        .post(endpoint)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await?;
    let status = resp.status().as_u16();
    if status == 429 {
        return Err(ListError::RateLimited);
    }
    if status == 400 || status == 401 {
        return Err(ListError::OAuth("exchange".into()));
    }
    if !(200..300).contains(&status) {
        return Err(ListError::Http(status));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| ListError::Parse(e.without_url().to_string()))?;
    token_from_mal_json(&v, now)
}

/// Exchange the MAL authorization code (PKCE `plain`: the verifier is the challenge).
pub async fn mal_exchange_code(
    client: &reqwest::Client,
    client_id: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
    now: i64,
) -> Result<Token, ListError> {
    let form = [
        ("client_id", client_id),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", verifier),
        ("redirect_uri", redirect_uri),
    ];
    mal_token_request(client, MAL_TOKEN, &form, now).await
}

/// Refresh a MAL token. A rejected refresh token is `Unauthorized` (the user must connect again).
pub async fn mal_refresh(
    client: &reqwest::Client,
    client_id: &str,
    refresh_token: &str,
    now: i64,
) -> Result<Token, ListError> {
    let form = [
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    match mal_token_request(client, MAL_TOKEN, &form, now).await {
        Err(ListError::OAuth(_)) => Err(ListError::Unauthorized),
        r => r,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_values_have_the_rfc_shape() {
        let v = pkce_verifier();
        assert_eq!(v.len(), 64);
        assert!(
            v.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
        assert_ne!(v, pkce_verifier());
        // RFC 7636 appendix B
        assert_eq!(
            pkce_challenge_s256("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(new_state().len(), 32);
    }

    #[test]
    fn callback_classification() {
        assert_eq!(classify("/favicon.ico", "s"), Outcome::Ignore);
        assert_eq!(classify("/callback", "s"), Outcome::Forward);
        assert_eq!(
            classify("/callback?code=abc&state=s", "s"),
            Outcome::Done(Callback {
                code_or_token: "abc".into(),
                state: Some("s".into())
            })
        );
        assert_eq!(
            classify("/callback?code=abc&state=x", "s"),
            Outcome::Failed("state".into())
        );
        assert_eq!(
            classify("/callback?code=abc", "s"),
            Outcome::Failed("state".into())
        );
        // implicit grant forwarded by the page: state optional, but must match when present
        assert_eq!(
            classify(
                "/callback?access_token=t%2B1&token_type=Bearer&expires_in=31536000&forwarded=1",
                "s"
            ),
            Outcome::Done(Callback {
                code_or_token: "t+1".into(),
                state: None
            })
        );
        assert_eq!(
            classify("/callback?access_token=t&state=bad&forwarded=1", "s"),
            Outcome::Failed("state".into())
        );
        assert_eq!(
            classify("/callback?error=access_denied&state=s", "s"),
            Outcome::Failed("denied".into())
        );
        assert_eq!(
            classify("/callback?error=no_token&forwarded=1", "s"),
            Outcome::Failed("invalid_callback".into())
        );
        assert_eq!(
            classify("/callback?error=server_error", "s"),
            Outcome::Failed("provider:server_error".into())
        );
        assert_eq!(
            classify("/callback?forwarded=1", "s"),
            Outcome::Failed("invalid_callback".into())
        );
    }

    #[test]
    fn request_line_parsing() {
        assert_eq!(
            request_target("GET /callback?code=1 HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/callback?code=1")
        );
        assert_eq!(request_target("POST /callback HTTP/1.1\r\n"), None);
        assert_eq!(request_target(""), None);
    }

    #[test]
    fn authorize_urls_carry_the_parameters() {
        let a = anilist_authorize_url("123", "st");
        assert!(a.starts_with(ANILIST_AUTHORIZE));
        assert!(a.contains("client_id=123") && a.contains("response_type=token") && a.contains("state=st"));
        let m = mal_authorize_url("abc", "verifier", "st", "http://127.0.0.1:47410/callback");
        assert!(m.contains("code_challenge=verifier") && m.contains("code_challenge_method=plain"));
        assert!(m.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A47410%2Fcallback"));
    }

    #[test]
    fn mal_token_json_maps_expiry() {
        let v = serde_json::json!({ "token_type": "Bearer", "expires_in": 2_678_400, "access_token": "a", "refresh_token": "r" });
        let t = token_from_mal_json(&v, 1000).unwrap();
        assert_eq!(t.access_token, "a");
        assert_eq!(t.refresh_token.as_deref(), Some("r"));
        assert_eq!(t.expires_at, Some(1000 + 2_678_400));
        assert!(matches!(
            token_from_mal_json(&serde_json::json!({}), 0),
            Err(ListError::OAuth(_))
        ));
        assert_eq!(anilist_token("x", 10).expires_at, Some(10 + ANILIST_TOKEN_SECS));
    }

    #[tokio::test]
    async fn loopback_round_trip_with_forwarded_fragment() {
        let lb = bind_loopback(false).await.unwrap();
        let port = lb.port().unwrap();
        assert!(LOOPBACK_PORTS.contains(&port));
        let waiter = tokio::spawn(async move { lb.accept_within("s", Duration::from_secs(10)).await });
        let get = |path: &'static str| async move {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        };
        assert!(get("/favicon.ico").await.starts_with("HTTP/1.1 404"));
        assert!(get("/callback").await.contains("location.replace"));
        assert!(
            get("/callback?access_token=tok&forwarded=1")
                .await
                .starts_with("HTTP/1.1 200")
        );
        let cb = waiter.await.unwrap().unwrap();
        assert_eq!(cb.code_or_token, "tok");
    }

    #[tokio::test]
    async fn loopback_times_out() {
        let lb = bind_loopback(false).await.unwrap();
        let r = lb.accept_within("s", Duration::from_millis(50)).await;
        assert!(matches!(r, Err(ListError::OAuth(c)) if c == "timeout"));
    }
}
