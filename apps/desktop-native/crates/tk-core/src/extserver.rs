//! Client of a paired extension server (wave B, worker **j**; docs/redesign/13 section 3.2 is the API; checklist
//! `ext.server-client.*`, `ext.ecosystems.via-server`; classic `core/extensions/server/client.ts`,
//! `pairingLink.ts`, `hostGuard.ts`). UI-free; tk-app's `live/ext_server.rs` / `live/ext_catalog.rs` own the state.
//! Same surface the Android `:core:extension-server` client has (docs/redesign/13 section 3.10), so both apps talk
//! to `server/` and to `tk-server` alike.
//!
//! * Host guard (D6): `https` anywhere, plain `http` only for local-network hosts (RFC 1918, CGNAT / Tailscale,
//!   link-local, ULA, loopback, `*.local`), never embedded credentials.
//! * Envelope: every JSON answer is `{ success, data, error: { code, message } }`; a body without `success` is taken
//!   as the data itself (older builds).
//! * `/resolve` is NDJSON: each line is a full snapshot `{ sources, subtitles, statuses }` (not a delta); a line
//!   `{ "done": true }` (tk-server) ends the stream. The last snapshot wins.

use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use crate::error::ExtServerError;

pub const API_PREFIX: &str = "/api/server/v1";
/// Plain requests (health, auth, sources, invoke).
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// The server caps a resolution at 25 s; the stream needs a little slack.
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(35);
/// Health probe used to mark a server offline quickly (`ext.server-client.fallback`).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
/// Cap of one NDJSON resolution (same as the classic client).
pub const RESOLVE_CAP_BYTES: usize = 8 * 1024 * 1024;
/// Label this device shows in the PC's device list.
pub const CLIENT_LABEL: &str = "Tatakai Next (desktop)";

/// `GET /health` body (fields the client needs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub protocol_version: u32,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub pairing_required: bool,
    #[serde(default)]
    pub setup_required: bool,
    #[serde(default)]
    pub access_password_required: bool,
}

/// Token answer of `/auth/pair` and `/auth/login`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub token: String,
    #[serde(default = "viewer")]
    pub role: String,
    #[serde(default)]
    pub device_id: String,
}

fn viewer() -> String {
    "viewer".to_owned()
}

/// One source a server offers (`GET /sources`, `sourceDescriptor` of `server/src/extensions.mjs`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceDescriptor {
    /// `<serverId>/<row id>` (`~read` / `~novel` for the reading variants).
    pub id: String,
    #[serde(default)]
    pub extension_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub ecosystem: String,
    #[serde(default = "watch")]
    pub kind: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub available: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub languages: Vec<String>,
}

fn watch() -> String {
    "watch".to_owned()
}

fn yes() -> bool {
    true
}

/// A parsed pairing / connection link or typed address.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerLink {
    /// Normalized origin (`http://192.168.1.10:8099`).
    pub server: String,
    pub code: Option<String>,
    pub password: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Host guard and link parsing (classic hostGuard.ts / pairingLink.ts)
// ---------------------------------------------------------------------------------------------

fn private_v4(a: u8, b: u8) -> bool {
    a == 127 || a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168) || (a == 169 && b == 254) || (a == 100 && (64..=127).contains(&b))
}

/// True when `host` (a URL host name, IPv6 with or without brackets) is a local-network host.
pub fn is_local_network_host(raw: &str) -> bool {
    let host = raw.trim().to_ascii_lowercase();
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.');
    if host.is_empty() {
        return false;
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return true;
    }
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        let o = v4.octets();
        return private_v4(o[0], o[1]);
    }
    if let Ok(v6) = host.parse::<std::net::Ipv6Addr>() {
        if v6.is_loopback() {
            return true;
        }
        let first = v6.segments()[0];
        // fe80::/10 link-local, fc00::/7 unique local (Tailscale uses fd7a:115c:a1e0::/48).
        return (first & 0xffc0) == 0xfe80 || (first & 0xfe00) == 0xfc00;
    }
    // mDNS names need at least one label before `.local`.
    host.strip_suffix(".local").is_some_and(|rest| {
        !rest.is_empty() && rest.split('.').all(|l| !l.is_empty() && l.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    })
}

/// Whether a server (or media) URL may be used: `https` with a host, or `http` to a local-network host; never
/// embedded credentials.
pub fn is_allowed_server_url(u: &str) -> bool {
    let Ok(url) = Url::parse(u) else { return false };
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    match url.scheme() {
        "https" => url.host_str().is_some_and(|h| !h.is_empty()),
        "http" => url.host_str().is_some_and(is_local_network_host),
        _ => false,
    }
}

/// Validate a typed address and return its origin (`http://` is assumed for local-network hosts without a scheme,
/// `https://` otherwise; the `/server/` dashboard path is accepted).
pub fn normalize_server_origin(value: &str) -> Result<String, ExtServerError> {
    let raw = value.trim();
    if raw.is_empty() {
        return Err(ExtServerError::Refused("invalid_url".into()));
    }
    let has_scheme = raw.split_once("://").is_some_and(|(s, _)| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'.' | b'-')));
    let raw = if has_scheme {
        raw.to_owned()
    } else {
        let host = raw.split(['/', '?', '#']).next().unwrap_or("");
        let host_only = match host.rsplit_once(':') {
            Some((h, port)) if !h.contains(':') && port.bytes().all(|c| c.is_ascii_digit()) => h,
            _ => host,
        };
        format!("{}{raw}", if is_local_network_host(host_only) { "http://" } else { "https://" })
    };
    let url = Url::parse(&raw).map_err(|_| ExtServerError::Refused("invalid_url".into()))?;
    let path_ok = matches!(url.path(), "" | "/" | "/server" | "/server/");
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !path_ok
    {
        return Err(ExtServerError::Refused("invalid_url".into()));
    }
    let origin = url.origin().ascii_serialization();
    if !is_allowed_server_url(&origin) {
        return Err(ExtServerError::Refused("insecure_host".into()));
    }
    Ok(origin)
}

/// One-time pairing codes: non-empty, at most 128 chars of `[A-Za-z0-9_-]`.
pub fn valid_pairing_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 128 && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Parse `tatakai://extensionserver/connect?server=<origin>&code=<code>` or a plain server address.
pub fn parse_server_link(input: &str) -> Result<ServerLink, ExtServerError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ExtServerError::Refused("invalid_link".into()));
    }
    if input.len() >= 8 && input[..8].eq_ignore_ascii_case("tatakai:") {
        let url = Url::parse(input).map_err(|_| ExtServerError::Refused("invalid_link".into()))?;
        let path = format!("/{}{}", url.host_str().unwrap_or("").to_ascii_lowercase(), url.path());
        if path.trim_end_matches('/') != "/extensionserver/connect" {
            return Err(ExtServerError::Refused("invalid_link".into()));
        }
        let param = |k: &str| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.trim().to_owned()).filter(|v| !v.is_empty());
        let server = param("server").ok_or_else(|| ExtServerError::Refused("invalid_link".into()))?;
        let server = normalize_server_origin(&server).map_err(|e| match e {
            ExtServerError::Refused(c) if c == "insecure_host" => ExtServerError::Refused(c),
            _ => ExtServerError::Refused("invalid_link".into()),
        })?;
        let code = param("code").ok_or_else(|| ExtServerError::Refused("invalid_link".into()))?;
        if !valid_pairing_code(&code) {
            return Err(ExtServerError::Refused("invalid_link".into()));
        }
        return Ok(ServerLink { server, code: Some(code), password: None });
    }
    Ok(ServerLink { server: normalize_server_origin(input)?, code: None, password: None })
}

/// A fresh local id for a server row / add-on / repository.
pub fn new_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

// ---------------------------------------------------------------------------------------------
// Envelope / errors
// ---------------------------------------------------------------------------------------------

/// `{ success, data, error }` -> data, or the error of the call at `path` with HTTP `status`.
pub fn read_envelope(path: &str, status: u16, body: &str) -> Result<Value, ExtServerError> {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let (success, data, code, message) = match &parsed {
        Some(Value::Object(o)) if o.get("success").is_some_and(Value::is_boolean) => (
            o.get("success").and_then(Value::as_bool).unwrap_or(false),
            o.get("data").cloned().unwrap_or(Value::Null),
            o.get("error").and_then(|e| e.get("code")).and_then(Value::as_str).map(str::to_owned),
            o.get("error").and_then(|e| e.get("message")).and_then(Value::as_str).map(str::to_owned),
        ),
        Some(v) => ((200..300).contains(&status), v.clone(), None, None),
        None => (false, Value::Null, None, None),
    };
    if (200..300).contains(&status) && success {
        return Ok(data);
    }
    Err(failure(path, status, code.as_deref(), message))
}

/// Classic `failure()`: 401 means different things per route.
fn failure(path: &str, status: u16, code: Option<&str>, message: Option<String>) -> ExtServerError {
    if status == 401 {
        return ExtServerError::Refused(
            match path {
                "/auth/login" if code == Some("pairing_required") => "pairing_required",
                "/auth/login" => "invalid_password",
                "/auth/pair" => "pairing_expired",
                _ if code == Some("device_not_authorized") => "device_not_authorized",
                _ => "authentication_required",
            }
            .to_owned(),
        );
    }
    match code {
        Some(c @ ("login_throttled" | "setup_required" | "pairing_expired" | "pairing_required" | "forbidden")) => ExtServerError::Refused(c.to_owned()),
        _ if status == 0 => ExtServerError::Parse(message.unwrap_or_else(|| "invalid response".into())),
        _ => match message {
            Some(m) if !m.is_empty() && !(200..300).contains(&status) => ExtServerError::Parse(format!("http {status}: {m}")),
            _ if (200..300).contains(&status) => ExtServerError::Parse("invalid response".into()),
            _ => ExtServerError::Http(status),
        },
    }
}

/// Last full snapshot of an NDJSON `/resolve` body (lines that are not JSON objects are skipped; `{done: true}`
/// ends it). `None` when no line carried `sources`.
pub fn last_snapshot(ndjson: &str) -> Option<Value> {
    let mut last = None;
    for line in ndjson.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v.get("done").and_then(Value::as_bool) == Some(true) && v.get("sources").is_none() {
            break;
        }
        // tk-server may wrap lines in the envelope too.
        let v = match v.get("data") {
            Some(d) if d.get("sources").is_some() => d.clone(),
            _ => v,
        };
        if v.get("sources").is_some_and(Value::is_array) {
            last = Some(v);
        }
    }
    last
}

// ---------------------------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------------------------

pub struct ExtServerClient {
    base: Url,
    token: Option<String>,
    http: reqwest::Client,
}

impl ExtServerClient {
    /// `origin` must pass [`is_allowed_server_url`].
    pub fn new(http: reqwest::Client, origin: &str, token: Option<String>) -> Result<Self, ExtServerError> {
        if !is_allowed_server_url(origin) {
            return Err(ExtServerError::Refused("insecure_host".into()));
        }
        let base = Url::parse(origin).map_err(|_| ExtServerError::Refused("invalid_url".into()))?;
        Ok(Self { base, token: token.filter(|t| !t.is_empty()), http })
    }

    pub fn origin(&self) -> String {
        self.base.origin().ascii_serialization()
    }

    fn url(&self, path: &str) -> Result<Url, ExtServerError> {
        self.base.join(&format!("{API_PREFIX}{path}")).map_err(|_| ExtServerError::Refused("invalid_url".into()))
    }

    async fn request(&self, path: &str, body: Option<Value>, timeout: Duration) -> Result<Value, ExtServerError> {
        let url = self.url(path)?;
        let mut rb = match &body {
            Some(b) => self.http.post(url).json(b),
            None => self.http.get(url),
        };
        rb = rb.timeout(timeout).header("accept", "application/json");
        if let Some(t) = &self.token {
            rb = rb.bearer_auth(t);
        }
        let resp = rb.send().await.map_err(|e| ExtServerError::Network(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let text = resp.text().await.map_err(|e| ExtServerError::Network(e.without_url().to_string()))?;
        read_envelope(path, status, &text)
    }

    pub async fn health(&self) -> Result<Health, ExtServerError> {
        self.health_within(REQUEST_TIMEOUT).await
    }

    /// `GET /health` with a short timeout (offline detection).
    pub async fn health_within(&self, timeout: Duration) -> Result<Health, ExtServerError> {
        let v = self.request("/health", None, timeout).await?;
        let h: Health = serde_json::from_value(v).map_err(|e| ExtServerError::Parse(e.to_string()))?;
        if h.protocol_version != 1 {
            return Err(ExtServerError::Refused("incompatible".into()));
        }
        Ok(h)
    }

    pub async fn pair(&self, code: &str, label: &str) -> Result<Token, ExtServerError> {
        if !valid_pairing_code(code) {
            return Err(ExtServerError::Refused("invalid_link".into()));
        }
        let v = self.request("/auth/pair", Some(json!({ "code": code, "label": label })), REQUEST_TIMEOUT).await?;
        serde_json::from_value(v).map_err(|e| ExtServerError::Parse(e.to_string()))
    }

    pub async fn login_viewer(&self, password: &str, label: &str) -> Result<Token, ExtServerError> {
        let body = json!({ "password": password, "role": "viewer", "label": label });
        let v = self.request("/auth/login", Some(body), REQUEST_TIMEOUT).await?;
        serde_json::from_value(v).map_err(|e| ExtServerError::Parse(e.to_string()))
    }

    pub async fn logout(&self) -> Result<(), ExtServerError> {
        if self.token.is_none() {
            return Err(ExtServerError::NotPaired(self.origin()));
        }
        self.request("/auth/logout", Some(json!({})), Duration::from_secs(5)).await.map(|_| ())
    }

    pub async fn sources(&self) -> Result<Vec<SourceDescriptor>, ExtServerError> {
        let v = self.request("/sources", None, REQUEST_TIMEOUT).await?;
        serde_json::from_value(v).map_err(|e| ExtServerError::Parse(e.to_string()))
    }

    /// `POST /invoke` (`browse`, `search`, `info`, `episodes`, `resolve`, `chapters`, `pages`, `novelChapters`,
    /// `novelContent`).
    pub async fn invoke(&self, source_id: &str, op: &str, args: Value) -> Result<Value, ExtServerError> {
        let body = json!({ "sourceId": source_id, "op": op, "args": args });
        self.request("/invoke", Some(body), REQUEST_TIMEOUT).await
    }

    /// `POST /resolve` (NDJSON progressive): the final `{ sources, subtitles, statuses }` snapshot over every
    /// enabled watch source of the server. `args` = `{ anilistId?, title, episode, dub }`.
    pub async fn resolve(&self, args: Value) -> Result<Value, ExtServerError> {
        let url = self.url("/resolve")?;
        let mut rb = self.http.post(url).json(&args).timeout(RESOLVE_TIMEOUT).header("accept", "application/x-ndjson");
        if let Some(t) = &self.token {
            rb = rb.bearer_auth(t);
        }
        let mut resp = rb.send().await.map_err(|e| ExtServerError::Network(e.without_url().to_string()))?;
        let status = resp.status().as_u16();
        let ndjson = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|c| c.contains("ndjson"));
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| ExtServerError::Network(e.without_url().to_string()))? {
            if buf.len() + chunk.len() > RESOLVE_CAP_BYTES {
                return Err(ExtServerError::Parse("server response exceeded the limit".into()));
            }
            buf.extend_from_slice(&chunk);
        }
        let text = String::from_utf8_lossy(&buf);
        if !(200..300).contains(&status) || !ndjson {
            // Aggregated JSON answer (or an error envelope).
            return read_envelope("/resolve", status, &text);
        }
        last_snapshot(&text).ok_or_else(|| ExtServerError::Parse("empty resolution".into()))
    }
}

/// Normalize the sources of a `/resolve` snapshot into the extension-route shape tk-ext maps
/// (`{ sources: [{ url, type, quality, headers, subtitles, audioLanguage, providerName, server, infoHash, ... }] }`).
/// Plain-http media URLs must share the server's origin (its `/media/*` proxy); others are dropped (classic
/// `isAllowedServerMediaUrl`).
pub fn normalize_snapshot(origin: &str, snapshot: &Value) -> Value {
    let same_origin = |u: &str| Url::parse(u).is_ok_and(|p| p.origin().ascii_serialization() == origin);
    let sources: Vec<Value> = snapshot["sources"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let url = s["url"].as_str().or_else(|| s["file"].as_str()).unwrap_or("").trim().to_owned();
            let lower = url.to_ascii_lowercase();
            let torrent = s["isTorrent"].as_bool() == Some(true) || s["infoHash"].is_string() || lower.starts_with("magnet:");
            if !torrent {
                if lower.starts_with("http://") && !same_origin(&url) {
                    return None;
                }
                if !lower.starts_with("http://") && !lower.starts_with("https://") {
                    return None;
                }
            }
            let mut o = s.as_object().cloned().unwrap_or_default();
            o.insert("url".into(), json!(url));
            let ty = s["type"].as_str().or_else(|| s["sourceType"].as_str()).map(str::to_ascii_lowercase);
            let ty = match ty.as_deref() {
                Some(t @ ("hls" | "mp4" | "dash" | "torrent" | "embed")) => t.to_owned(),
                _ if torrent => "torrent".to_owned(),
                _ if lower.split(['?', '#']).next().is_some_and(|p| p.ends_with(".m3u8")) => "hls".to_owned(),
                _ if lower.split(['?', '#']).next().is_some_and(|p| p.ends_with(".mpd")) => "dash".to_owned(),
                _ => "mp4".to_owned(),
            };
            o.insert("type".into(), json!(ty));
            if o.get("server").and_then(Value::as_str).is_none_or(str::is_empty) {
                let label = ["sourceName", "source", "extensionName", "providerName"].iter().find_map(|k| s[*k].as_str().filter(|v| !v.is_empty()));
                if let Some(l) = label {
                    o.insert("server".into(), json!(l));
                }
            }
            if o.get("audioLanguage").is_none()
                && let Some(l) = s["language"].as_str()
            {
                o.insert("audioLanguage".into(), json!(l));
            }
            Some(Value::Object(o))
        })
        .collect();
    json!({ "sources": sources })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_guard_matches_the_classic_rules() {
        for ok in ["192.168.1.2", "10.0.0.1", "172.20.1.1", "100.100.1.1", "169.254.1.1", "127.0.0.1", "localhost", "pc.local", "[fd7a:115c:a1e0::1]", "::1"] {
            assert!(is_local_network_host(ok), "{ok}");
        }
        for bad in ["8.8.8.8", "example.com", ".local", "172.32.0.1", "2001:db8::1", ""] {
            assert!(!is_local_network_host(bad), "{bad}");
        }
        assert!(is_allowed_server_url("http://192.168.1.2:8099"));
        assert!(is_allowed_server_url("https://tk.example.org"));
        assert!(!is_allowed_server_url("http://example.org"));
        assert!(!is_allowed_server_url("https://u:p@example.org"));
        assert!(!is_allowed_server_url("ftp://192.168.1.2"));
    }

    #[test]
    fn origins_are_normalized() {
        assert_eq!(normalize_server_origin("192.168.1.10:8099").unwrap(), "http://192.168.1.10:8099");
        assert_eq!(normalize_server_origin("https://tk.example.org/server/").unwrap(), "https://tk.example.org");
        assert_eq!(normalize_server_origin("tk.example.org").unwrap(), "https://tk.example.org");
        assert!(matches!(normalize_server_origin("http://example.org"), Err(ExtServerError::Refused(c)) if c == "insecure_host"));
        assert!(normalize_server_origin("http://10.0.0.2/x?y=1").is_err());
        assert!(normalize_server_origin("  ").is_err());
    }

    #[test]
    fn pairing_links_parse() {
        let l = parse_server_link("tatakai://extensionserver/connect?server=http%3A%2F%2F10.0.0.2%3A8099&code=ab_C-12").unwrap();
        assert_eq!(l.server, "http://10.0.0.2:8099");
        assert_eq!(l.code.as_deref(), Some("ab_C-12"));
        assert!(parse_server_link("tatakai://extensionserver/connect?server=http%3A%2F%2F10.0.0.2&code=bad%20code").is_err());
        assert!(parse_server_link("tatakai://other/path?server=x&code=1").is_err());
        let plain = parse_server_link("http://192.168.0.5:8099").unwrap();
        assert_eq!((plain.server.as_str(), plain.code), ("http://192.168.0.5:8099", None));
    }

    #[test]
    fn envelopes_and_errors() {
        assert_eq!(read_envelope("/health", 200, r#"{"success":true,"data":{"a":1}}"#).unwrap(), json!({"a":1}));
        assert_eq!(read_envelope("/health", 200, r#"{"a":1}"#).unwrap(), json!({"a":1}));
        assert!(matches!(read_envelope("/auth/pair", 401, r#"{"success":false}"#), Err(ExtServerError::Refused(c)) if c == "pairing_expired"));
        assert!(matches!(read_envelope("/auth/login", 401, r#"{"success":false,"error":{"code":"pairing_required"}}"#), Err(ExtServerError::Refused(c)) if c == "pairing_required"));
        assert!(matches!(read_envelope("/sources", 401, ""), Err(ExtServerError::Refused(c)) if c == "authentication_required"));
        assert!(matches!(read_envelope("/sources", 502, "<html>"), Err(ExtServerError::Http(502))));
    }

    #[test]
    fn ndjson_keeps_the_last_snapshot() {
        let body = "{\"sources\":[],\"statuses\":[]}\n{\"sources\":[{\"url\":\"https://a/b.m3u8\"}]}\n{\"done\":true}\n";
        assert_eq!(last_snapshot(body).unwrap()["sources"].as_array().unwrap().len(), 1);
        assert!(last_snapshot("garbage\n").is_none());
    }

    #[test]
    fn snapshot_sources_are_guarded_and_typed() {
        let snap = json!({ "sources": [
            { "url": "http://10.0.0.2:8099/api/server/v1/media/t/x.m3u8", "sourceName": "Alpha", "quality": "1080p" },
            { "url": "http://evil.example/x.mp4" },
            { "url": "https://cdn.example/x.mp4", "language": "es" },
            { "infoHash": "0123456789abcdef0123456789abcdef01234567", "url": "", "title": "[Grp] Show - 01" },
        ]});
        let out = normalize_snapshot("http://10.0.0.2:8099", &snap);
        let list = out["sources"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["type"], "hls");
        assert_eq!(list[0]["server"], "Alpha");
        assert_eq!(list[1]["type"], "mp4");
        assert_eq!(list[1]["audioLanguage"], "es");
        assert_eq!(list[2]["type"], "torrent");
    }
}
