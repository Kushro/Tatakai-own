//! tk-server: the native extension server of Tatakai Next (wave B, worker **k**; plan 14 section 2; checklist
//! `server.*`; docs/redesign/13 section 3.2 is the API it must stay compatible with). It replaces the Node `server/`
//! of the classic app for the **`.kai` extensions installed on this PC**: a phone pairs with a one-time code and then
//! calls `/api/server/v1/sources`, `/invoke`, `/resolve` and the `/media/*` proxy exactly as it does against `server/`
//! (same JSON, same error catalogue), so the Android client of docs/redesign/13 section 3.10 needs no change.
//!
//! Out of scope by decision (plan 14 D4 + checklist `server.interpreted-extensions`): Aniyomi / Mihon / CloudStream /
//! Sora / Mangayomi / Seanime bundles are **not** interpreted here; Next is a *client* of those ecosystems through a
//! paired `server/`. No dashboard SPA (`server.dashboard-spa` out), no cloudflared / WARP (plan 14 section 0.1 #14).
//!
//! Shape (final; k fills the bodies):
//! * [`ServerConfig`] from `Settings::server` + the data dir; [`start`] binds `0.0.0.0:<port>` and returns a
//!   [`ServerHandle`] (`stop`, `status`, `create_pairing`, `devices`, `revoke`, `set_viewer_password`, `logs`,
//!   `clear_cache`).
//! * [`state`]: `state.json` under `<data>/server/` (server id, device sessions, viewer password hash, settings) with
//!   atomic writes; [`auth`]: bearer tokens, pairing codes (6 digits, 300 s, single use, <= 50 live), argon2id for the
//!   viewer password; [`routes`]: the axum router (one handler per classic route, listed in [`routes::ROUTES`]);
//!   [`proxy`]: `GET | HEAD /media/*` with the stored header ticket and an on-disk cache; [`logs`]: ring buffer for
//!   `GET /logs` and the app's Server page.
//! * The extension host is the app's `tk_ext::ExtensionHost` (shared `Arc`): `/sources` lists its enabled
//!   extensions as `SourceDescriptor`s (ecosystem `tatakai`, `~read` / `~novel` variants for manga / novel
//!   capabilities), `/invoke` maps `{ sourceId, op, args }` to `ExtensionHost::call` routes (`custom/home`,
//!   `custom/search`, `custom/info`, `chapters`, `pages`, `novelChapters`, `novelContent`, ...), `/resolve` to
//!   `ExtensionHost::resolve_candidates` streamed as NDJSON (one source per line, `{ "done": true }` last), with
//!   stream URLs rewritten to the `/media/<ticket>` proxy when they carry headers.

pub mod api;
pub mod auth;
pub mod logs;
pub mod proxy;
pub mod routes;
pub mod state;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tk_ext::ExtensionHost;

pub use api::{ApiError, PROTOCOL_VERSION};

/// Path prefix of every route (docs/redesign/13).
pub const API_PREFIX: &str = "/api/server/v1";
/// Classic default port (`server/` `ExtensionHostPortSetting`).
pub const DEFAULT_PORT: u16 = 8099;

/// What the app passes from `Settings::server` and its paths.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// `<data>/server` (state.json, media cache).
    pub dir: PathBuf,
    pub port: u16,
    /// Shown in `/health` and to phones; empty = host name.
    pub name: String,
    /// Phones must pair (one-time code) unless a viewer password is set (docs/redesign/13 D4b `requireDeviceToken`).
    pub require_pairing: bool,
    pub proxy_enabled: bool,
    /// Extra CORS origins (the LAN phones send none; kept for third-party clients).
    pub allowed_origins: Vec<String>,
    /// Per-source resolve timeout.
    pub timeout: std::time::Duration,
    pub concurrency: usize,
    /// FlareSolverr endpoint for Cloudflare-protected fetches of the proxy (`server.flaresolverr`).
    pub flaresolverr_url: Option<String>,
}

impl ServerConfig {
    pub fn new(dir: PathBuf, port: u16) -> Self {
        Self {
            dir,
            port,
            name: String::new(),
            require_pairing: true,
            proxy_enabled: true,
            allowed_origins: Vec::new(),
            timeout: std::time::Duration::from_secs(45),
            concurrency: 4,
            flaresolverr_url: None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("state: {0}")]
    State(#[from] state::StateError),
    #[error("not running")]
    NotRunning,
}

/// A running server. Dropping it does not stop the task: call [`ServerHandle::stop`].
pub struct ServerHandle {
    #[allow(dead_code)]
    cfg: ServerConfig,
    #[allow(dead_code)]
    ext: Arc<ExtensionHost>,
    #[allow(dead_code)]
    state: Arc<state::ServerState>,
    #[allow(dead_code)]
    logs: Arc<logs::LogBuffer>,
    shutdown: tokio_util::sync::CancellationToken,
    pub addr: SocketAddr,
    pub started_unix: i64,
}

impl ServerHandle {
    pub async fn stop(&self) {
        self.shutdown.cancel();
    }
    /// Snapshot for the app's Server page (`ServerStatusVm`): k maps it in `tk-app/src/live/server.rs`.
    pub fn status(&self) -> Status {
        Status { addr: self.addr, started_unix: self.started_unix, server_id: self.state.server_id(), devices: self.state.device_count() }
    }
    /// A new single-use pairing code (`POST /auth/pairing` of the classic server; `createPairing()` of docs/13 D4).
    pub fn create_pairing(&self, origin: &str) -> Result<auth::Pairing, ServerError> {
        Ok(self.state.create_pairing(origin))
    }
    pub fn devices(&self) -> Vec<state::DeviceRecord> {
        self.state.devices()
    }
    pub fn revoke(&self, device_id: &str) -> Result<(), ServerError> {
        self.state.revoke(device_id).map_err(ServerError::State)
    }
    /// `Some` sets (argon2id hash), `None` clears (`server.viewer-password`).
    pub fn set_viewer_password(&self, password: Option<&str>) -> Result<(), ServerError> {
        self.state.set_viewer_password(password).map_err(ServerError::State)
    }
    pub fn logs(&self, after: u64) -> (Vec<logs::LogEntry>, u64) {
        self.logs.since(after)
    }
    pub async fn clear_cache(&self) {
        proxy::clear_cache(&self.cfg.dir).await;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub addr: SocketAddr,
    pub started_unix: i64,
    pub server_id: Option<String>,
    pub devices: u32,
}

/// Bind and serve. Contract stub: k replaces the body (`TcpListener::bind`, `axum::serve` with the router of
/// [`routes::router`] and graceful shutdown on the token). Until then it reports `Bind` with `Unsupported`.
pub async fn start(cfg: ServerConfig, ext: Arc<ExtensionHost>) -> Result<ServerHandle, ServerError> {
    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let _ = (ext, cfg);
    Err(ServerError::Bind { addr, source: std::io::Error::new(std::io::ErrorKind::Unsupported, "tk-server is not wired yet (wave B, worker k)") })
}
