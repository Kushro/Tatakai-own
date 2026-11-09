//! The axum router of `/api/server/v1` (worker **k**; classic `server/src/core.mjs` `dispatch` + `handle`). One
//! handler per route of [`ROUTES`]; everything answers through the classic envelope (`crate::api::ok` /
//! `crate::api::Failure`). The outer middleware checks the `Origin` (CORS, `origin_denied`), answers `OPTIONS`, adds
//! the security headers and writes one `request` entry per call to the log ring (not for `/health`, `/media`,
//! `/logs`).
//!
//! Route table (method, path, who may call; `owner` = an admin token from `POST /setup` / `POST /auth/login` with
//! the `X-Tatakai-Bootstrap` secret of `state.json` (local tools; the app itself drives the server in process),
//! `viewer` = paired phone (or anyone when pairing is off and no viewer password is set), `public` = no token):
//! * `GET /health` public -> `{ protocolVersion: 1, serverId, name, platform: "desktop", pairingRequired, dashboardPath: null, ... }`
//! * `POST /setup` (`X-Tatakai-Bootstrap`) -> admin token
//! * `POST /auth/login { role, password, label? }` -> `{ token, role, deviceId }`; viewer without password: `401 pairing_required`
//! * `POST /auth/pair { code, label? }` -> `{ token, role: "viewer", deviceId }`; `401 pairing_expired`
//! * `POST /auth/logout` viewer|owner -> `{ success: true }`
//! * `POST /auth/pairing` owner -> `{ code, expiresIn: 300, url, deepLink }`; `409 pairing_limit`
//! * `POST /auth/password { password | null }` owner -> `{ success: true }`
//! * `GET /devices` owner -> `[{ id, label, role, createdAt, lastSeenAt, current }]`; `DELETE /devices/{id}` owner
//! * `GET /settings` owner / `POST /settings` owner (`409 managed_by_app`: the app's Settings > Server & network rule)
//! * `GET /extensions` viewer -> the PC's `.kai` extensions `[{ id, name, version, type, sources: [...] }]` (`server.api.extensions`)
//! * `POST /extensions { url }` owner -> install a `.kai` from an https URL; `DELETE /extensions/{id}` owner
//! * `GET /repositories` viewer / `POST /repositories { url, name? }` owner
//! * `GET /sources` viewer -> `SourceDescriptor[]` (`~read` / `~novel` variants) sorted by priority (`server.api.sources`)
//! * `POST /invoke { sourceId, op, args, progressive? }` viewer -> JSON (`server.api.invoke`)
//! * `POST /resolve { anilistId, malId?, title, titles?, episode }` viewer -> NDJSON progressive when `Accept:
//!   application/x-ndjson` (every line a complete snapshot, the last one with `"done": true`), else one JSON snapshot
//! * `GET | HEAD /media/{ticket}/{*path}` ticket holder -> proxied media; `POST /media/refresh { ticket }` viewer
//! * `GET /logs?after=` owner -> `{ entries, cursor }`; `GET /cache` owner -> `{ lookupEntries, mediaEntries, mediaBytes, maxMediaBytes }`; `DELETE /cache` owner
//! * `GET /export` owner / `POST /import` owner (repositories, schema 1)
//! * `GET /diagnostics` owner -> `{ platform, protocolVersion, extensionCount, capabilities }`
//! * `POST /native` -> `404 not_found` (Node-only feature of the classic server; documented as unsupported)

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{ConnectInfo, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{MethodFilter, delete, get, on, post};
use serde_json::{Value, json};

use crate::API_PREFIX;
use crate::api::{ApiError, Failure, ok};
use crate::logs::{LogBuffer, LogEntry};
use crate::proxy::{LOCAL_SESSION, MediaProxy};
use crate::state::{DeviceRecord, ServerState, StateError};

/// `(method, path)` the router serves (paths relative to [`API_PREFIX`]).
pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/health"),
    ("POST", "/setup"),
    ("POST", "/auth/login"),
    ("POST", "/auth/pair"),
    ("POST", "/auth/logout"),
    ("POST", "/auth/pairing"),
    ("POST", "/auth/password"),
    ("GET", "/devices"),
    ("DELETE", "/devices/{id}"),
    ("GET", "/settings"),
    ("POST", "/settings"),
    ("GET", "/extensions"),
    ("POST", "/extensions"),
    ("DELETE", "/extensions/{id}"),
    ("GET", "/repositories"),
    ("POST", "/repositories"),
    ("GET", "/sources"),
    ("POST", "/invoke"),
    ("POST", "/resolve"),
    ("GET", "/media/{ticket}/{*path}"),
    ("HEAD", "/media/{ticket}/{*path}"),
    ("POST", "/media/refresh"),
    ("GET", "/logs"),
    ("GET", "/cache"),
    ("DELETE", "/cache"),
    ("GET", "/export"),
    ("POST", "/import"),
    ("GET", "/diagnostics"),
];

/// Origins of the native shells always accepted (classic `nativeOrigins`; the hosted web origins are not).
pub const NATIVE_ORIGINS: &[&str] = &["capacitor://localhost", "http://localhost", "https://localhost"];
/// Largest JSON body accepted (the classic server takes up to 14 MiB for imports; phones send a few KiB).
const MAX_BODY: usize = 4 * 1024 * 1024;
/// Largest `.kai` downloaded by `POST /extensions`.
const MAX_KAI: usize = 20 * 1024 * 1024;

/// Bounded cache of unproxied `/invoke` answers (classic `lookup-cache.mjs`): 64 entries, 2 MiB each, 5 minutes.
#[derive(Default)]
pub struct LookupCache {
    inner: Mutex<(HashMap<String, (Instant, Value)>, VecDeque<String>)>,
}

impl LookupCache {
    const MAX_ENTRIES: usize = 64;
    const MAX_ENTRY_BYTES: usize = 2 * 1024 * 1024;
    const TTL: std::time::Duration = std::time::Duration::from_secs(300);
    const EMPTY_TTL: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn get(&self, key: &str) -> Option<Value> {
        let g = self.inner.lock().ok()?;
        let (at, v) = g.0.get(key)?;
        let empty = v.as_array().is_some_and(Vec::is_empty);
        let ttl = if empty { Self::EMPTY_TTL } else { Self::TTL };
        (at.elapsed() < ttl).then(|| v.clone())
    }

    pub fn put(&self, key: String, value: Value) {
        if value.to_string().len() > Self::MAX_ENTRY_BYTES {
            return;
        }
        let Ok(mut g) = self.inner.lock() else { return };
        if g.0.insert(key.clone(), (Instant::now(), value)).is_none() {
            g.1.push_back(key);
        }
        while g.1.len() > Self::MAX_ENTRIES {
            if let Some(k) = g.1.pop_front() {
                g.0.remove(&k);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.0.values().filter(|(at, _)| at.elapsed() < Self::TTL).count()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.0.clear();
            g.1.clear();
        }
    }
}

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<crate::ServerConfig>,
    pub ext: Arc<tk_ext::ExtensionHost>,
    pub state: Arc<ServerState>,
    pub logs: Arc<LogBuffer>,
    /// Media tickets, cache and upstream client (`proxy.rs`).
    pub proxy: Arc<MediaProxy>,
    /// `/invoke` lookup cache.
    pub lookups: Arc<LookupCache>,
    pub started_unix: i64,
}

impl AppState {
    /// Name shown to phones: the configured one or this PC's host name.
    pub fn display_name(&self) -> String {
        let n = self.cfg.name.trim();
        if n.is_empty() { crate::host_name() } else { n.to_owned() }
    }

    fn pairing_required(&self) -> bool {
        self.cfg.require_pairing && !self.state.viewer_password_set()
    }

    /// Anonymous viewers are admitted only with pairing off and no viewer password.
    fn anonymous_allowed(&self) -> bool {
        !self.cfg.require_pairing && !self.state.viewer_password_set()
    }

    /// Whether a ticket minted for `session` may still be served to a caller at `peer`.
    pub fn session_ok(&self, session: &str, peer: Option<SocketAddr>) -> bool {
        if session == LOCAL_SESSION {
            return peer.is_some_and(|p| p.ip().is_loopback());
        }
        if session.starts_with("public:") {
            return self.anonymous_allowed() && session == self.state.public_session_id();
        }
        self.state.device(session).is_some()
    }

    fn log(&self, event: &str, route: Option<&str>, name: Option<String>, count: Option<u32>, duration_ms: Option<u32>, error: Option<String>) {
        self.logs.push(LogEntry {
            id: 0,
            at_unix: crate::auth::now_unix(),
            event: event.to_owned(),
            route: route.map(str::to_owned),
            name,
            count,
            duration_ms,
            error,
        });
    }
}

/// Peer address of the connection (absent in tests that call the router directly).
#[derive(Clone, Copy, Debug)]
struct Peer(Option<SocketAddr>);

/// Who is calling.
#[derive(Debug, Clone)]
enum Caller {
    Device(DeviceRecord),
    /// Anonymous viewer (`public:<epoch>`).
    Public(String),
}

impl Caller {
    fn session(&self) -> String {
        match self {
            Self::Device(d) => d.id.clone(),
            Self::Public(s) => s.clone(),
        }
    }
}

fn bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    v.strip_prefix("Bearer ").map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
}

fn authorize(st: &AppState, headers: &HeaderMap, owner: bool) -> Result<Caller, Failure> {
    match bearer(headers) {
        Some(token) => {
            let dev = st.state.authenticate(&token).ok_or(ApiError::DeviceNotAuthorized)?;
            if owner && dev.role != "admin" {
                return Err(ApiError::Forbidden.into());
            }
            Ok(Caller::Device(dev))
        }
        None if !owner && st.anonymous_allowed() => Ok(Caller::Public(st.state.public_session_id())),
        None => Err(ApiError::AuthenticationRequired.into()),
    }
}

fn parse_body(body: &Bytes) -> Result<Value, Failure> {
    if body.len() > MAX_BODY {
        return Err(Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"));
    }
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_json"))
}

fn state_err(e: StateError) -> Failure {
    match e {
        StateError::PairingLimit => ApiError::PairingLimit.into(),
        StateError::DeviceLimit => Failure::new(StatusCode::CONFLICT, "device_limit"),
        StateError::RepositoryLimit => Failure::new(StatusCode::CONFLICT, "repository_limit"),
        StateError::NoDevice(_) => ApiError::NotFound.into(),
        StateError::Password(m) => Failure::new(StatusCode::BAD_REQUEST, "password_length").with_message(m),
        other => {
            tracing::warn!(error = %other, "server state error");
            ApiError::Internal.into()
        }
    }
}

/// `http://<Host>` of the request (what phones used to reach us), else the bound address.
fn base_url(st: &AppState, headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty() && h.len() <= 255 && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']')));
    match host {
        Some(h) => format!("http://{h}"),
        None => format!("http://127.0.0.1:{}", st.cfg.port),
    }
}

fn token_answer(dev: &DeviceRecord, token: &str) -> Response {
    ok(json!({ "token": token, "role": dev.role, "deviceId": dev.id }))
}

// ---- public / auth ----------------------------------------------------------------------------------------------

async fn health(State(st): State<AppState>) -> Response {
    let now = crate::auth::now_unix();
    ok(json!({
        "protocolVersion": crate::PROTOCOL_VERSION,
        "serverId": st.state.server_id(),
        "name": st.display_name(),
        "platform": "desktop",
        "desktop": true,
        "logoUrl": Value::Null,
        "startedAt": time::OffsetDateTime::from_unix_timestamp(st.started_unix).ok().and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok()),
        "uptimeSeconds": (now - st.started_unix).max(0),
        "setupRequired": false,
        "accessPasswordRequired": st.state.viewer_password_set(),
        "adminPasswordRequired": false,
        "pairingRequired": st.pairing_required(),
        "dashboardPath": Value::Null,
        "capabilities": [{ "ecosystem": "tatakai", "available": true, "reason": "Tatakai .kai extensions installed on this PC" }],
        "proxy": { "hls": true, "range": true, "enabled": st.cfg.proxy_enabled, "maxResponseBytes": Value::Null },
    }))
}

async fn setup(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    let secret = headers.get("x-tatakai-bootstrap").and_then(|v| v.to_str().ok()).unwrap_or("");
    if !st.state.bootstrap_ok(secret) {
        return Err(Failure::new(StatusCode::FORBIDDEN, "bootstrap_required"));
    }
    let (dev, token) = st.state.issue("admin", "Owner").map_err(state_err)?;
    st.log("setup", Some("/setup"), Some(dev.label.clone()), None, None, None);
    Ok(token_answer(&dev, &token))
}

async fn login(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    let input = parse_body(&body)?;
    let role = if input["role"].as_str() == Some("admin") { "admin" } else { "viewer" };
    let label = input["label"].as_str().unwrap_or("Device");
    if st.state.throttled(role) {
        return Err(Failure::new(StatusCode::TOO_MANY_REQUESTS, "login_throttled"));
    }
    if role == "admin" {
        let secret = headers.get("x-tatakai-bootstrap").and_then(|v| v.to_str().ok()).unwrap_or("");
        if !st.state.bootstrap_ok(secret) {
            st.state.note_failure(role);
            return Err(Failure::new(StatusCode::FORBIDDEN, "owner_key_required").with_message("Use the setup key of state.json for owner access"));
        }
    } else if st.state.viewer_password_set() {
        let password = input["password"].as_str().unwrap_or("");
        if !st.state.verify_viewer_password(password) {
            st.state.note_failure(role);
            st.log("login_failed", Some("/auth/login"), None, None, None, Some("invalid_password".into()));
            return Err(Failure::new(StatusCode::UNAUTHORIZED, "invalid_password"));
        }
    } else if st.cfg.require_pairing {
        return Err(Failure::from(ApiError::PairingRequired).with_message("Pair this device from the server owner (QR code or link)"));
    }
    let (dev, token) = st.state.issue(role, label).map_err(state_err)?;
    st.log("login", Some("/auth/login"), Some(dev.label.clone()), None, None, None);
    Ok(token_answer(&dev, &token))
}

async fn pair(State(st): State<AppState>, body: Bytes) -> Result<Response, Failure> {
    let input = parse_body(&body)?;
    if st.state.throttled("pair") {
        return Err(Failure::new(StatusCode::TOO_MANY_REQUESTS, "login_throttled"));
    }
    let code = match &input["code"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    };
    let label = input["label"].as_str().unwrap_or("Device");
    match st.state.pair(&code, label).map_err(state_err)? {
        Some((dev, token)) => {
            st.log("pairing", Some("/auth/pair"), Some(dev.label.clone()), None, None, None);
            Ok(token_answer(&dev, &token))
        }
        None => {
            st.state.note_failure("pair");
            st.log("pairing_failed", Some("/auth/pair"), None, None, None, Some("pairing_expired".into()));
            Err(ApiError::PairingExpired.into())
        }
    }
}

async fn logout(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    if let Caller::Device(d) = authorize(&st, &headers, false)? {
        st.state.revoke(&d.id).map_err(state_err)?;
        st.log("logout", Some("/auth/logout"), Some(d.label), None, None, None);
    }
    Ok(ok(json!({ "success": true })))
}

async fn create_pairing(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let p = st.state.try_create_pairing(&base_url(&st, &headers)).map_err(state_err)?;
    st.log("pairing_created", Some("/auth/pairing"), None, None, None, None);
    Ok(ok(p))
}

async fn set_password(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let input = parse_body(&body)?;
    if input["role"].as_str().is_some_and(|r| r != "viewer") {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unsupported_role").with_message("Only the viewer password exists on desktop"));
    }
    let password = input["password"].as_str().filter(|p| !p.is_empty());
    st.state.set_viewer_password(password).map_err(state_err)?;
    st.log("viewer_password", Some("/auth/password"), None, None, None, None);
    Ok(ok(json!({ "success": true })))
}

// ---- owner ------------------------------------------------------------------------------------------------------

async fn list_devices(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    let me = authorize(&st, &headers, true)?.session();
    let rows: Vec<Value> = st
        .state
        .devices()
        .into_iter()
        .map(|d| json!({ "id": d.id, "label": d.label, "role": d.role, "createdAt": d.created_at, "lastSeenAt": d.last_seen_at, "current": d.id == me }))
        .collect();
    Ok(ok(rows))
}

async fn revoke_device(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    st.state.revoke(&id).map_err(state_err)?;
    st.log("revoked", Some("/devices"), None, None, None, None);
    Ok(ok(json!({ "success": true })))
}

fn settings_json(st: &AppState) -> Value {
    json!({
        "name": st.display_name(),
        "requirePairing": st.cfg.require_pairing,
        "proxyEnabled": st.cfg.proxy_enabled,
        "allowedOrigins": st.cfg.allowed_origins,
        "timeoutMs": st.cfg.timeout.as_millis() as u64,
        "concurrency": st.cfg.concurrency,
        "languages": [],
        "cacheSeconds": 300,
        "mediaCacheMb": crate::proxy::MAX_CACHE_BYTES / (1024 * 1024),
        "flaresolverr": st.cfg.flaresolverr_url.is_some(),
    })
}

async fn get_settings(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    Ok(ok(settings_json(&st)))
}

async fn post_settings(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    Err(Failure::new(StatusCode::CONFLICT, "managed_by_app").with_message("Change these settings in Tatakai: Settings > Server & network"))
}

/// One `SourceDescriptor` row (plus its `~read` / `~novel` variants), as `GET /sources` lists them.
pub fn descriptors(server_id: &str, ext: &tk_core::InstalledExtension, manifest: Option<&tk_ext::Manifest>) -> Vec<Value> {
    let available = ext.compat != tk_core::Compat::Unsupported;
    let priority = manifest.and_then(|m| m.priority).unwrap_or(0.0);
    let base = json!({
        "id": format!("{server_id}/{}", ext.id),
        "extensionId": ext.id,
        "name": ext.name,
        "ecosystem": "tatakai",
        "enabled": ext.enabled,
        "priority": priority,
        "languages": [],
        "kind": "watch",
        "sourceType": ext.kind,
        "version": ext.version,
        "available": available,
        "reason": ext.compat_reason.clone().unwrap_or_else(|| if available { "Tatakai .kai extension".to_owned() } else { "Not supported by this PC's extension host".to_owned() }),
    });
    let mut out = vec![base.clone()];
    let caps = manifest.map(|m| m.capabilities.clone()).unwrap_or_default();
    let mut variant = |suffix: &str, label: &str, kind: &str| {
        let mut v = base.clone();
        v["id"] = json!(format!("{server_id}/{}~{suffix}", ext.id));
        v["name"] = json!(format!("{} · {label}", ext.name));
        v["kind"] = json!(kind);
        out.push(v);
    };
    if caps.iter().any(|c| c == "manga") {
        variant("read", "Manga", "read");
    }
    if caps.iter().any(|c| c == "novel") {
        variant("novel", "Novels", "novel");
    }
    out
}

/// The manifest of an installed extension, read from the host's own directory (`<root>/<id>/manifest.json`).
pub fn manifest_of(host: &tk_ext::ExtensionHost, id: &str) -> Option<tk_ext::Manifest> {
    let text = std::fs::read_to_string(host.root().join(id).join("manifest.json")).ok()?;
    tk_ext::Manifest::parse(&text).ok()
}

fn sorted_extensions(st: &AppState) -> Vec<(tk_core::InstalledExtension, Option<tk_ext::Manifest>)> {
    let mut v: Vec<_> = st.ext.list().into_iter().map(|e| {
        let m = manifest_of(&st.ext, &e.id);
        (e, m)
    }).collect();
    v.sort_by(|a, b| {
        let pa = a.1.as_ref().and_then(|m| m.priority).unwrap_or(0.0);
        let pb = b.1.as_ref().and_then(|m| m.priority).unwrap_or(0.0);
        pb.total_cmp(&pa).then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
    });
    v
}

async fn list_extensions(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, false)?;
    let sid = st.state.server_id().unwrap_or_default();
    let rows: Vec<Value> = sorted_extensions(&st)
        .into_iter()
        .map(|(e, m)| {
            let sources = descriptors(&sid, &e, m.as_ref());
            json!({
                "id": e.id, "manifestId": e.id, "ecosystem": "tatakai", "name": e.name, "version": e.version,
                "type": e.kind, "kind": e.kind, "enabled": e.enabled,
                "compat": format!("{:?}", e.compat).to_ascii_lowercase(), "compatReason": e.compat_reason,
                "priority": m.as_ref().and_then(|m| m.priority).unwrap_or(0.0),
                "capabilities": m.as_ref().map(|m| m.capabilities.clone()).unwrap_or_default(),
                "languages": [], "hasPreferences": false, "runtime": "interpreter", "sources": sources,
            })
        })
        .collect();
    Ok(ok(rows))
}

async fn install_extension(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let input = parse_body(&body)?;
    let url = input["url"].as_str().unwrap_or("").trim().to_owned();
    let parsed = url::Url::parse(&url).map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_url"))?;
    if parsed.scheme() != "https" {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_url").with_message("Use an https:// link to a .kai package"));
    }
    let (resp, _) = st
        .proxy
        .upstream
        .send(reqwest::Method::GET, parsed, &[])
        .await
        .map_err(|e| Failure::new(StatusCode::BAD_GATEWAY, "bundle_download_failed").with_message(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(Failure::new(StatusCode::BAD_GATEWAY, "bundle_download_failed"));
    }
    let mut resp = resp;
    let mut data = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|_| Failure::new(StatusCode::BAD_GATEWAY, "bundle_download_failed"))? {
        if data.len() + chunk.len() > MAX_KAI {
            return Err(Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "response_too_large"));
        }
        data.extend_from_slice(&chunk);
    }
    let dir = st.cfg.dir.join("downloads");
    tokio::fs::create_dir_all(&dir).await.map_err(|_| Failure::from(ApiError::Internal))?;
    let file = dir.join(format!("{}.kai", crate::auth::new_code()));
    tokio::fs::write(&file, &data).await.map_err(|_| Failure::from(ApiError::Internal))?;
    let host = st.ext.clone();
    let f2 = file.clone();
    let installed = tokio::task::spawn_blocking(move || host.install_kai(&f2)).await.map_err(|_| Failure::from(ApiError::Internal))?;
    let _ = tokio::fs::remove_file(&file).await;
    let e = installed.map_err(|e| Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid_extension").with_message(e.to_string()))?;
    st.lookups.clear();
    st.log("extension_installed", Some("/extensions"), Some(e.name.clone()), None, None, None);
    Ok(ok(json!({ "id": e.id, "name": e.name, "version": e.version, "type": e.kind, "enabled": e.enabled })))
}

async fn remove_extension(State(st): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    st.ext.remove(&id).map_err(|_| Failure::new(StatusCode::NOT_FOUND, "extension_not_found"))?;
    st.lookups.clear();
    st.log("extension_removed", Some("/extensions"), Some(id), None, None, None);
    Ok(ok(json!({ "success": true })))
}

async fn list_repositories(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, false)?;
    Ok(ok(st.state.repositories()))
}

async fn add_repository(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let input = parse_body(&body)?;
    let url = url::Url::parse(input["url"].as_str().unwrap_or("")).map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_url"))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "invalid_url"));
    }
    let name = input["name"].as_str().map(str::to_owned).unwrap_or_else(|| url.host_str().unwrap_or("Repository").to_owned());
    Ok(ok(st.state.add_repository(&name, url.as_str()).map_err(state_err)?))
}

async fn sources(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, false)?;
    let sid = st.state.server_id().unwrap_or_default();
    let rows: Vec<Value> =
        sorted_extensions(&st).into_iter().filter(|(e, _)| e.enabled).flat_map(|(e, m)| descriptors(&sid, &e, m.as_ref())).collect();
    Ok(ok(rows))
}

// ---- invoke -----------------------------------------------------------------------------------------------------

/// Which `.kai` route an `/invoke` op runs (classic op names and the host route names are both accepted).
pub fn invoke_route(op: &str, novel: bool) -> Option<&'static str> {
    Some(match op {
        "browse" | "custom/home" => "custom/home",
        "search" if novel => "novel/search",
        "search" | "custom/search" => "custom/search",
        "info" | "episodes" | "custom/info" => "custom/info",
        "resolve" | "custom/watch" => "custom/watch",
        "custom/read" => "custom/read",
        "chapters" | "manga/chapters" => "manga/chapters",
        "pages" | "manga/pages" => "manga/pages",
        "novelChapters" | "novel/chapters" => "novel/chapters",
        "novelContent" | "novel/content" => "novel/content",
        "novel/search" => "novel/search",
        "providers" => "providers",
        _ => return None,
    })
}

/// `<serverId>/<extId>[~read|~novel]` -> `(extId, novel variant)`.
pub fn parse_source_id(source_id: &str, server_id: &str) -> (String, bool) {
    let s = source_id.strip_prefix(&format!("{server_id}/")).unwrap_or(source_id);
    let novel = s.ends_with("~novel");
    let id = s.trim_end_matches("~novel").trim_end_matches("~read").to_owned();
    (id, novel)
}

const PROXIED_KEYS: [&str; 6] = ["url", "imageUrl", "image", "poster", "thumbnail", "icon"];
const NESTED_KEYS: [&str; 10] = ["subtitles", "pages", "sources", "streams", "metas", "videos", "results", "items", "sections", "chapters"];

fn headers_of(v: &Value) -> Option<Vec<(String, String)>> {
    v.get("headers")?.as_object().map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect())
}

/// Rewrite every media / image URL of an extension answer through `mint(url, headers)` (classic `proxify`): an
/// object's own `headers` (else the parent's, minus credentials when the origin changes) travel with the ticket;
/// `headers`, `originalUrl` and `behaviorHints` are removed; embed / custom page URLs are left as they are. Pure.
pub fn proxify(value: &Value, inherited: Option<(&str, &[(String, String)])>, mint: &mut dyn FnMut(&str, &[(String, String)]) -> String) -> Value {
    match value {
        Value::Array(a) => Value::Array(a.iter().map(|x| proxify(x, inherited, mint)).collect()),
        Value::Object(o) => {
            let own = headers_of(value);
            let parent_url = inherited.map(|(u, _)| u.to_owned());
            let parent_headers: Vec<(String, String)> = inherited.map(|(_, h)| h.to_vec()).unwrap_or_default();
            let embed = matches!(o.get("sourceType").or_else(|| o.get("type")).and_then(Value::as_str), Some("embed" | "custom"))
                || o.get("isEmbed").and_then(Value::as_bool) == Some(true);
            let mut out = o.clone();
            for key in PROXIED_KEYS {
                let Some(u) = o.get(key).and_then(Value::as_str) else { continue };
                if !(u.starts_with("http://") || u.starts_with("https://")) || (key == "url" && embed) {
                    continue;
                }
                let headers = match &own {
                    Some(h) => h.clone(),
                    None => match &parent_url {
                        Some(p) if !same_origin(p, u) => crate::upstream::strip_credentials(&parent_headers),
                        _ => parent_headers.clone(),
                    },
                };
                out.insert(key.to_owned(), Value::String(mint(u, &headers)));
            }
            out.remove("headers");
            out.remove("originalUrl");
            out.remove("behaviorHints");
            let my_url = o.get("url").and_then(Value::as_str).map(str::to_owned);
            let my_headers = own.clone().unwrap_or_else(|| parent_headers.clone());
            for key in NESTED_KEYS {
                if let Some(child) = o.get(key) {
                    let ctx = match &my_url {
                        Some(u) => Some((u.as_str(), my_headers.as_slice())),
                        None => inherited,
                    };
                    out.insert(key.to_owned(), proxify(child, ctx, mint));
                }
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    match (url::Url::parse(a), url::Url::parse(b)) {
        (Ok(x), Ok(y)) => tk_core::net::origin_of(&x) == tk_core::net::origin_of(&y),
        _ => false,
    }
}

fn mint_for<'a>(st: &'a AppState, base: &'a str, session: &'a str) -> impl FnMut(&str, &[(String, String)]) -> String + 'a {
    move |u: &str, h: &[(String, String)]| st.proxy.mint(base, u, h, session, None)
}

fn ext_failure(e: tk_ext::ExtError) -> Failure {
    match e {
        tk_ext::ExtError::NotInstalled(_) => Failure::new(StatusCode::NOT_FOUND, "source_not_found"),
        tk_ext::ExtError::RouteNotFound(r) | tk_ext::ExtError::RouteNotAllowed(r) => {
            Failure::new(StatusCode::UNPROCESSABLE_ENTITY, "unsupported_operation").with_message(r)
        }
        other => Failure::from(ApiError::Upstream(other.to_string())),
    }
}

async fn invoke(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    let caller = authorize(&st, &headers, false)?;
    let input = parse_body(&body)?;
    let sid = st.state.server_id().unwrap_or_default();
    let (ext_id, novel) = parse_source_id(input["sourceId"].as_str().unwrap_or(""), &sid);
    let op = input["op"].as_str().unwrap_or("");
    let route = invoke_route(op, novel).ok_or_else(|| Failure::new(StatusCode::FORBIDDEN, "data_operation_required"))?;
    let info = st.ext.get(&ext_id).filter(|e| e.enabled).ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, "source_not_found"))?;
    let args = if input["args"].is_object() { input["args"].clone() } else { json!({}) };
    let cacheable = route != "custom/watch";
    let key = crate::auth::token_hash(&json!([ext_id, info.version, route, args]).to_string());
    let started = Instant::now();
    let raw = match cacheable.then(|| st.lookups.get(&key)).flatten() {
        Some(v) => v,
        None => {
            let res = tokio::time::timeout(st.cfg.timeout, st.ext.call(&ext_id, route, args))
                .await
                .map_err(|_| Failure::new(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"))?;
            match res {
                Ok(v) => {
                    if cacheable {
                        st.lookups.put(key, v.clone());
                    }
                    v
                }
                Err(e) => {
                    st.log("source_failed", Some("/invoke"), Some(info.name.clone()), None, Some(elapsed_ms(started)), Some(e.to_string()));
                    return Err(ext_failure(e));
                }
            }
        }
    };
    let raw = if route == "manga/pages" { pages_as_objects(raw) } else { raw };
    let base = base_url(&st, &headers);
    let session = caller.session();
    let result = if st.cfg.proxy_enabled {
        let mut mint = mint_for(&st, &base, &session);
        proxify(&raw, None, &mut mint)
    } else {
        raw
    };
    let progressive = input["progressive"].as_bool() == Some(true) && matches!(route, "manga/chapters" | "novel/chapters");
    Ok(ok(if progressive { json!({ "chapters": result, "partial": false }) } else { result }))
}

/// `pages` answers may be plain URL strings: the classic server turns them into `{ imageUrl }`.
fn pages_as_objects(v: Value) -> Value {
    let map = |a: Vec<Value>| Value::Array(a.into_iter().map(|p| if let Value::String(s) = p { json!({ "imageUrl": s }) } else { p }).collect());
    match v {
        Value::Array(a) => map(a),
        Value::Object(mut o) => {
            if let Some(Value::Array(p)) = o.remove("pages") {
                o.insert("pages".into(), map(p));
            }
            Value::Object(o)
        }
        other => other,
    }
}

fn elapsed_ms(t: Instant) -> u32 {
    t.elapsed().as_millis().min(u128::from(u32::MAX)) as u32
}

// ---- resolve ----------------------------------------------------------------------------------------------------

/// Route params of the `stream` / `torrent` routes from a `/resolve` body (same keys as `tk_ext`'s own resolver).
pub fn resolve_params(input: &Value, timeout_ms: u64) -> Value {
    let mut titles: Vec<String> = input["titles"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    if let Some(t) = input["title"].as_str().filter(|t| !t.is_empty())
        && !titles.iter().any(|x| x == t)
    {
        titles.insert(0, t.to_owned());
    }
    let mut p = json!({
        "anilistId": input["anilistId"].as_u64().unwrap_or(0),
        "titles": titles,
        "episode": input["episode"].as_u64().unwrap_or(1).max(1),
        "stream": "0",
        "resolution": "1080p",
        "providerOptions": { "timeoutMs": timeout_ms.min(25_000) },
    });
    if let Some(m) = input["malId"].as_u64() {
        p["malId"] = json!(m);
    }
    p
}

/// One source of an extension answer with the server's provider fields (classic `/resolve` `sources.push`).
pub fn annotate_source(mut s: Value, provider_key: &str, ext_name: &str, server_name: &str) -> Value {
    let Some(o) = s.as_object_mut() else { return s };
    if !o.contains_key("sourceType")
        && let Some(t) = o.get("type").cloned()
    {
        o.insert("sourceType".into(), t);
    }
    if !o.contains_key("sourceType") && (o.contains_key("infoHash") || o.contains_key("magnet")) {
        o.insert("sourceType".into(), json!("torrent"));
    }
    let source_name = ["sourceName", "providerName", "server", "source"]
        .iter()
        .find_map(|k| o.get(*k).and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_else(|| ext_name.to_owned());
    o.insert("sourceName".into(), json!(source_name));
    o.insert("providerKey".into(), json!(provider_key));
    o.insert("extensionId".into(), json!(provider_key));
    o.insert("extensionName".into(), json!(ext_name));
    o.insert("extensionServerName".into(), json!(server_name));
    o.insert("providerName".into(), json!(format!("{ext_name} · {server_name}")));
    o.insert("ecosystem".into(), json!("tatakai"));
    s
}

struct ResolveJob {
    id: String,
    name: String,
    routes: Vec<&'static str>,
}

fn resolve_jobs(st: &AppState) -> Vec<ResolveJob> {
    sorted_extensions(st)
        .into_iter()
        .filter(|(e, _)| e.enabled && e.compat != tk_core::Compat::Unsupported)
        .filter_map(|(e, m)| {
            let m = m?;
            let bundle = std::fs::read_to_string(st.ext.root().join(&e.id).join("bundle.js")).unwrap_or_default();
            let routes = tk_ext::candidate_routes(&m, tk_ext::exposes_torrent_all(&bundle));
            (!routes.is_empty()).then(|| ResolveJob { id: e.id, name: e.name, routes })
        })
        .collect()
}

#[derive(Default)]
struct Snapshot {
    sources: Vec<Value>,
    statuses: Vec<Value>,
}

impl Snapshot {
    fn json(&self, done: bool) -> Value {
        let mut v = json!({ "sources": self.sources, "subtitles": [], "statuses": self.statuses });
        if done {
            v["done"] = json!(true);
        }
        v
    }
}

async fn run_resolve(st: AppState, input: Value, base: String, session: String, publish: Option<tokio::sync::mpsc::UnboundedSender<Bytes>>) -> Value {
    use futures_util::StreamExt;
    let sid = st.state.server_id().unwrap_or_default();
    let server_name = st.display_name();
    let jobs = resolve_jobs(&st);
    let params = resolve_params(&input, st.cfg.timeout.as_millis() as u64);
    let snap = Arc::new(Mutex::new(Snapshot {
        sources: Vec::new(),
        statuses: jobs
            .iter()
            .map(|j| json!({ "provider": format!("{sid}/{}", j.id), "name": j.name, "ecosystem": "tatakai", "kind": "anime", "status": "loading", "count": 0, "durationMs": 0 }))
            .collect(),
    }));
    let send = |done: bool| {
        if let (Some(tx), Ok(s)) = (&publish, snap.lock()) {
            let mut line = s.json(done).to_string();
            line.push('\n');
            let _ = tx.send(Bytes::from(line));
        }
    };
    send(false);
    let concurrency = st.cfg.concurrency.clamp(1, 8);
    let tasks = jobs.iter().enumerate().map(|(i, job)| {
        let st = st.clone();
        let params = params.clone();
        let snap = snap.clone();
        let sid = sid.clone();
        let server_name = server_name.clone();
        let base = base.clone();
        let session = session.clone();
        async move {
            let started = Instant::now();
            let provider = format!("{sid}/{}", job.id);
            let mut found: Vec<Value> = Vec::new();
            let mut error: Option<String> = None;
            for route in &job.routes {
                match tokio::time::timeout(st.cfg.timeout, st.ext.call(&job.id, route, params.clone())).await {
                    Ok(Ok(body)) => found.extend(body["sources"].as_array().cloned().unwrap_or_default()),
                    Ok(Err(e)) => error = Some(e.to_string()),
                    Err(_) => error = Some("timeout".into()),
                }
            }
            let mut seen = std::collections::HashSet::new();
            let fresh: Vec<Value> = found
                .into_iter()
                .filter(|s| seen.insert(json!([s["url"], s["infoHash"], s["magnet"], s["quality"], s["server"], s["fileIndex"]]).to_string()))
                .take(500)
                .collect();
            let count = fresh.len() as u32;
            let proxied: Vec<Value> = fresh
                .into_iter()
                .map(|s| {
                    let s = if st.cfg.proxy_enabled {
                        let mut mint = mint_for(&st, &base, &session);
                        proxify(&s, None, &mut mint)
                    } else {
                        s
                    };
                    annotate_source(s, &provider, &job.name, &server_name)
                })
                .collect();
            let duration = elapsed_ms(started);
            if let Ok(mut s) = snap.lock() {
                let room = 2000usize.saturating_sub(s.sources.len());
                s.sources.extend(proxied.into_iter().take(room));
                if let Some(row) = s.statuses.get_mut(i) {
                    row["count"] = json!(count);
                    row["durationMs"] = json!(duration);
                    row["status"] = json!(match (count, &error) {
                        (0, Some(_)) => "error",
                        (0, None) => "empty",
                        (_, Some(_)) => "partial",
                        _ => "ok",
                    });
                    if error.is_some() {
                        row["error"] = json!("source_failed");
                    }
                }
            }
            let failed = error.is_some() && count == 0;
            st.log(if failed { "source_failed" } else { "source_completed" }, Some("/resolve"), Some(job.name.clone()), Some(count), Some(duration), error);
        }
    });
    let mut stream = futures_util::stream::iter(tasks).buffer_unordered(concurrency);
    while stream.next().await.is_some() {
        send(false);
    }
    send(true);
    snap.lock().map(|s| s.json(true)).unwrap_or_else(|_| json!({ "sources": [], "subtitles": [], "statuses": [], "done": true }))
}

async fn resolve(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    let caller = authorize(&st, &headers, false)?;
    let input = parse_body(&body)?;
    let base = base_url(&st, &headers);
    let session = caller.session();
    let ndjson = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("application/x-ndjson"));
    if !ndjson {
        return Ok(ok(run_resolve(st, input, base, session, None).await));
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    tokio::spawn(run_resolve(st, input, base, session, Some(tx)));
    let stream = futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|b| (Ok::<Bytes, std::io::Error>(b), rx)) });
    let mut r = Response::new(Body::from_stream(stream));
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/x-ndjson"));
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(r)
}

// ---- media ------------------------------------------------------------------------------------------------------

async fn media_any(st: AppState, method: Method, headers: HeaderMap, peer: Peer, ticket: String) -> Result<Response, Failure> {
    let base = base_url(&st, &headers);
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let check = |s: &str| st.session_ok(s, peer.0);
    st.proxy.serve(&method, &ticket, range.as_deref(), &base, &check).await
}

async fn media(
    State(st): State<AppState>,
    method: Method,
    headers: HeaderMap,
    axum::Extension(peer): axum::Extension<Peer>,
    Path((ticket, _path)): Path<(String, String)>,
) -> Result<Response, Failure> {
    media_any(st, method, headers, peer, ticket).await
}

async fn media_bare(
    State(st): State<AppState>,
    method: Method,
    headers: HeaderMap,
    axum::Extension(peer): axum::Extension<Peer>,
    Path(ticket): Path<String>,
) -> Result<Response, Failure> {
    media_any(st, method, headers, peer, ticket).await
}

/// `ticket` may be the bare id or a whole media URL.
fn ticket_id(raw: &str) -> String {
    let after = raw.split("/media/").nth(1).unwrap_or(raw);
    after.split(['/', '?']).next().unwrap_or("").to_owned()
}

async fn media_refresh(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    let caller = authorize(&st, &headers, false)?;
    let input = parse_body(&body)?;
    let t = st.proxy.ticket(&ticket_id(input["ticket"].as_str().unwrap_or(""))).ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, "media_not_found"))?;
    if t.session != caller.session() {
        return Err(Failure::new(StatusCode::FORBIDDEN, "media_owner_mismatch"));
    }
    let url = st.proxy.mint(&base_url(&st, &headers), &t.url, &t.headers, &t.session, None);
    Ok(ok(json!({ "url": url })))
}

// ---- logs, cache, export, diagnostics ---------------------------------------------------------------------------

async fn logs(State(st): State<AppState>, headers: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let after = q.get("after").and_then(|a| a.parse::<u64>().ok()).unwrap_or(0);
    let (entries, cursor) = st.logs.since(after);
    Ok(ok(json!({ "entries": entries, "cursor": cursor })))
}

fn cache_json(st: &AppState) -> Value {
    let (entries, bytes) = st.proxy.cache.stats();
    json!({ "lookupEntries": st.lookups.len(), "mediaEntries": entries, "mediaBytes": bytes, "maxMediaBytes": crate::proxy::MAX_CACHE_BYTES })
}

async fn get_cache(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    Ok(ok(cache_json(&st)))
}

async fn delete_cache(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    st.lookups.clear();
    st.proxy.cache.clear();
    crate::proxy::clear_cache(&st.cfg.dir).await;
    Ok(ok(json!({ "success": true })))
}

async fn export(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let exts: Vec<Value> = st.ext.list().into_iter().map(|e| json!({ "id": e.id, "name": e.name, "version": e.version, "type": e.kind, "enabled": e.enabled })).collect();
    Ok(ok(json!({ "schema": 1, "settings": settings_json(&st), "extensions": exts, "repositories": st.state.repositories() })))
}

async fn import(State(st): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let input = parse_body(&body)?;
    if input["schema"].as_u64() != Some(1) {
        return Err(Failure::new(StatusCode::BAD_REQUEST, "unsupported_schema"));
    }
    let mut added = 0u32;
    for r in input["repositories"].as_array().cloned().unwrap_or_default() {
        let Some(u) = r["url"].as_str().and_then(|u| url::Url::parse(u).ok()).filter(|u| u.scheme() == "https") else { continue };
        let name = r["name"].as_str().unwrap_or_else(|| u.host_str().unwrap_or("Repository")).to_owned();
        if st.state.add_repository(&name, u.as_str()).is_ok() {
            added += 1;
        }
    }
    Ok(ok(json!({ "repositories": added, "settings": settings_json(&st) })))
}

async fn diagnostics(State(st): State<AppState>, headers: HeaderMap) -> Result<Response, Failure> {
    authorize(&st, &headers, true)?;
    let (recent, _) = st.logs.since(0);
    let recent: Vec<_> = recent.into_iter().filter(|e| e.event.starts_with("source_")).rev().take(100).collect();
    Ok(ok(json!({
        "platform": "desktop",
        "protocolVersion": crate::PROTOCOL_VERSION,
        "extensionCount": st.ext.list().len(),
        "recent": recent,
        "cache": cache_json(&st),
        "capabilities": [{ "ecosystem": "tatakai", "available": true, "reason": "Tatakai .kai extensions installed on this PC" }],
    })))
}

async fn native() -> Failure {
    Failure::from(ApiError::NotFound).with_message("Native runtimes are a feature of the standalone Node server")
}

async fn not_found() -> Failure {
    Failure::new(StatusCode::NOT_FOUND, "route_not_found")
}

// ---- middleware -------------------------------------------------------------------------------------------------

/// `/auth/login` -> `/auth/login`, `/media/<t>/x` -> `/media`, unknown -> `/unknown` (classic `logs.mjs::logRoute`).
pub fn log_route(path: &str) -> String {
    let rest = path.strip_prefix(API_PREFIX).unwrap_or(path);
    let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
    let known = [
        "health", "setup", "auth", "extensions", "repositories", "sources", "invoke", "resolve", "settings", "devices", "diagnostics", "export",
        "import", "native", "cache", "media", "logs",
    ];
    match parts.first() {
        Some(&"auth") if parts.get(1).is_some_and(|p| ["login", "logout", "password", "pair", "pairing"].contains(p)) => format!("/auth/{}", parts[1]),
        Some(p) if known.contains(p) => format!("/{p}"),
        _ => "/unknown".to_owned(),
    }
}

/// Whether a browser `Origin` may call the API.
pub fn origin_allowed(origin: &str, own_host: Option<&str>, extra: &[String]) -> bool {
    if own_host.is_some_and(|h| origin == format!("http://{h}")) {
        return true;
    }
    NATIVE_ORIGINS.contains(&origin) || extra.iter().any(|o| o == origin)
}

async fn edge(State(st): State<AppState>, mut req: Request, next: Next) -> Response {
    let started = Instant::now();
    let path = req.uri().path().to_owned();
    let method = req.method().clone();
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    req.extensions_mut().insert(Peer(peer));
    let origin = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let host = req.headers().get(header::HOST).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let route = log_route(&path);
    let allowed = origin.as_deref().is_none_or(|o| origin_allowed(o, host.as_deref(), &st.cfg.allowed_origins));
    let mut resp = if !allowed {
        Failure::new(StatusCode::FORBIDDEN, "origin_denied").into_response()
    } else if method == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(req).await
    };
    let h = resp.headers_mut();
    if let (Some(o), true) = (&origin, allowed)
        && let Ok(v) = HeaderValue::from_str(o)
    {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
        h.insert(header::VARY, HeaderValue::from_static("Origin"));
    }
    h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("Authorization, Content-Type, Range, X-Tatakai-Bootstrap"));
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, HEAD, POST, PATCH, DELETE, OPTIONS"));
    h.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, HeaderValue::from_static("Content-Range, Accept-Ranges, Content-Length"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    if route != "/media" {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    }
    let status = resp.status();
    if !matches!(route.as_str(), "/health" | "/media" | "/logs") && method != Method::OPTIONS {
        st.log(
            "request",
            Some(&route),
            Some(method.to_string()),
            None,
            Some(elapsed_ms(started)),
            (status.is_client_error() || status.is_server_error()).then(|| status.as_u16().to_string()),
        );
    }
    resp
}

/// The router: every route of [`ROUTES`] (plus `/media/{ticket}` without a file name and `POST /native`) behind the
/// CORS / log middleware.
pub fn router(state: AppState) -> Router {
    let p = |s: &str| format!("{API_PREFIX}{s}");
    Router::new()
        .route(&p("/health"), get(health))
        .route(&p("/setup"), post(setup))
        .route(&p("/auth/login"), post(login))
        .route(&p("/auth/pair"), post(pair))
        .route(&p("/auth/logout"), post(logout))
        .route(&p("/auth/pairing"), post(create_pairing))
        .route(&p("/auth/password"), post(set_password))
        .route(&p("/devices"), get(list_devices))
        .route(&p("/devices/{id}"), delete(revoke_device))
        .route(&p("/settings"), get(get_settings).post(post_settings))
        .route(&p("/extensions"), get(list_extensions).post(install_extension))
        .route(&p("/extensions/{id}"), delete(remove_extension))
        .route(&p("/repositories"), get(list_repositories).post(add_repository))
        .route(&p("/sources"), get(sources))
        .route(&p("/invoke"), post(invoke))
        .route(&p("/resolve"), post(resolve))
        .route(&p("/media/refresh"), post(media_refresh))
        .route(&p("/media/{ticket}"), on(MethodFilter::GET.or(MethodFilter::HEAD), media_bare))
        .route(&p("/media/{ticket}/{*path}"), on(MethodFilter::GET.or(MethodFilter::HEAD), media))
        .route(&p("/logs"), get(logs))
        .route(&p("/cache"), get(get_cache).delete(delete_cache))
        .route(&p("/export"), get(export))
        .route(&p("/import"), post(import))
        .route(&p("/diagnostics"), get(diagnostics))
        .route(&p("/native"), post(native))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), edge))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_table_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for r in super::ROUTES {
            assert!(seen.insert(*r), "duplicate route {r:?}");
        }
        assert!(super::ROUTES.contains(&("POST", "/invoke")));
        assert_eq!(super::ROUTES.len(), 28);
    }

    #[test]
    fn invoke_ops_map_to_kai_routes() {
        assert_eq!(invoke_route("browse", false), Some("custom/home"));
        assert_eq!(invoke_route("custom/home", false), Some("custom/home"));
        assert_eq!(invoke_route("search", true), Some("novel/search"));
        assert_eq!(invoke_route("chapters", false), Some("manga/chapters"));
        assert_eq!(invoke_route("novelContent", false), Some("novel/content"));
        assert_eq!(invoke_route("stream", false), None);
        assert_eq!(invoke_route("eval", false), None);
        assert_eq!(parse_source_id("srv/toko~novel", "srv"), ("toko".to_owned(), true));
        assert_eq!(parse_source_id("srv/toko~read", "srv"), ("toko".to_owned(), false));
        assert_eq!(parse_source_id("toko", "srv"), ("toko".to_owned(), false));
    }

    #[test]
    fn proxify_mints_media_and_keeps_embeds() {
        let v = json!({ "sources": [
            { "url": "https://cdn.a/x.m3u8", "type": "hls", "headers": { "Referer": "https://site/" },
              "subtitles": [{ "url": "https://cdn.a/en.vtt" }, { "url": "https://other.b/es.vtt" }] },
            { "url": "https://embed.c/e/1", "type": "embed" },
        ], "poster": "https://img.d/p.jpg" });
        let mut minted: Vec<(String, Vec<(String, String)>)> = Vec::new();
        let mut mint = |u: &str, h: &[(String, String)]| {
            minted.push((u.to_owned(), h.to_vec()));
            format!("T{}", minted.len())
        };
        let out = proxify(&v, None, &mut mint);
        assert_eq!(out["poster"], "T1");
        assert_eq!(out["sources"][0]["url"], "T2");
        assert!(out["sources"][0].get("headers").is_none());
        assert_eq!(out["sources"][0]["subtitles"][0]["url"], "T3");
        assert_eq!(out["sources"][1]["url"], "https://embed.c/e/1");
        let referer = vec![("Referer".to_owned(), "https://site/".to_owned())];
        assert_eq!(minted[1].1, referer);
        assert_eq!(minted[2].1, referer, "same origin child inherits the parent's headers");
        assert_eq!(minted[3].1, vec![("referer".to_owned(), "https://site/".to_owned())], "cross origin child: cleaned, no credentials");
    }

    #[test]
    fn resolve_params_and_annotation() {
        let p = resolve_params(&json!({ "anilistId": 21, "malId": 21, "title": "One Piece", "titles": ["ワンピース"], "episode": 3 }), 45_000);
        assert_eq!(p["titles"], json!(["One Piece", "ワンピース"]));
        assert_eq!(p["episode"], 3);
        assert_eq!(p["malId"], 21);
        assert_eq!(p["providerOptions"]["timeoutMs"], 25_000);
        let s = annotate_source(json!({ "url": "T1", "type": "hls", "server": "Vidstream" }), "srv/toko", "Toko", "PC");
        assert_eq!(s["sourceType"], "hls");
        assert_eq!(s["sourceName"], "Vidstream");
        assert_eq!(s["providerName"], "Toko · PC");
        assert_eq!(s["providerKey"], "srv/toko");
        let t = annotate_source(json!({ "infoHash": "abc" }), "srv/nyaa", "Nyaa", "PC");
        assert_eq!(t["sourceType"], "torrent");
    }

    #[test]
    fn routes_origins_and_tickets() {
        assert_eq!(log_route("/api/server/v1/auth/pair"), "/auth/pair");
        assert_eq!(log_route("/api/server/v1/media/abc/x.ts"), "/media");
        assert_eq!(log_route("/api/server/v1/zzz"), "/unknown");
        assert!(origin_allowed("capacitor://localhost", None, &[]));
        assert!(origin_allowed("http://192.168.1.4:8099", Some("192.168.1.4:8099"), &[]));
        assert!(origin_allowed("https://tool.example", None, &["https://tool.example".to_owned()]));
        assert!(!origin_allowed("https://evil.example", Some("192.168.1.4:8099"), &[]));
        assert_eq!(ticket_id("http://h:1/api/server/v1/media/abc/index.m3u8"), "abc");
        assert_eq!(ticket_id("abc"), "abc");
        let cache = LookupCache::default();
        cache.put("k".into(), json!([1]));
        assert_eq!(cache.get("k"), Some(json!([1])));
        assert_eq!(cache.len(), 1);
        cache.clear();
        assert!(cache.is_empty());
    }
}
