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
/// Outbound requests (SSRF re-validation, HTTP / SOCKS5 proxy, FlareSolverr).
pub mod upstream;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    /// Wave B (k, private addition): outbound HTTP / SOCKS5 proxy of the media proxy (`server.proxy`), e.g.
    /// `socks5h://127.0.0.1:9050`. `None` = direct.
    pub outbound_proxy: Option<String>,
    /// Wave B (k, private addition): listen on 127.0.0.1 only. The app uses it for the loopback media proxy of mpv
    /// (`server.cors-proxy-local`) while the LAN server is off.
    pub loopback_only: bool,
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
            outbound_proxy: None,
            loopback_only: false,
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

/// This PC's host name (`/health` `name` when none is configured).
pub fn host_name() -> String {
    let from_env = ["COMPUTERNAME", "HOSTNAME"].iter().find_map(|k| std::env::var(k).ok()).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let from_file = || std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    from_env.or_else(from_file).map(|n| n.chars().filter(|c| !c.is_control()).take(60).collect()).unwrap_or_else(|| "Tatakai".to_owned())
}

/// Private LAN IPv4 addresses of this PC phones can reach (UDP "connect" trick: no packet is sent; same rule as
/// `tk_link::detect_lan_ips`).
pub fn lan_ips() -> Vec<Ipv4Addr> {
    let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", 0)) else { return Vec::new() };
    if sock.connect(("192.0.2.1", 9)).is_err() {
        return Vec::new();
    }
    match sock.local_addr() {
        Ok(SocketAddr::V4(a)) if (a.ip().is_private() || a.ip().is_link_local()) && !a.ip().is_loopback() => vec![*a.ip()],
        _ => Vec::new(),
    }
}

/// `http://<ip>:<port>` for every address in `ips` (what phones type or scan), loopback last. Pure.
pub fn urls_for(ips: &[Ipv4Addr], port: u16) -> Vec<String> {
    let mut out: Vec<String> = ips.iter().filter(|ip| !ip.is_loopback() && !ip.is_unspecified()).map(|ip| format!("http://{ip}:{port}")).collect();
    out.dedup();
    out.push(format!("http://127.0.0.1:{port}"));
    out
}

/// A running server. Dropping it does not stop the task: call [`ServerHandle::stop`].
pub struct ServerHandle {
    cfg: ServerConfig,
    #[allow(dead_code)]
    ext: Arc<ExtensionHost>,
    state: Arc<state::ServerState>,
    logs: Arc<logs::LogBuffer>,
    proxy: Arc<proxy::MediaProxy>,
    lookups: Arc<routes::LookupCache>,
    shutdown: tokio_util::sync::CancellationToken,
    task: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub addr: SocketAddr,
    pub started_unix: i64,
}

impl ServerHandle {
    /// Stop listening (graceful: requests in flight finish) and wait for the listener task, so the port is free
    /// when this returns.
    pub async fn stop(&self) {
        self.shutdown.cancel();
        let task = self.task.lock().ok().and_then(|mut t| t.take());
        if let Some(t) = task {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), t).await;
        }
    }
    /// Snapshot for the app's Server page (`ServerStatusVm`): k maps it in `tk-app/src/live/server.rs`.
    pub fn status(&self) -> Status {
        Status { addr: self.addr, started_unix: self.started_unix, server_id: self.state.server_id(), devices: self.state.device_count() }
    }
    /// A new single-use pairing code (`POST /auth/pairing` of the classic server; `createPairing()` of docs/13 D4).
    pub fn create_pairing(&self, origin: &str) -> Result<auth::Pairing, ServerError> {
        Ok(self.state.try_create_pairing(origin)?)
    }
    pub fn devices(&self) -> Vec<state::DeviceRecord> {
        self.state.devices()
    }
    pub fn revoke(&self, device_id: &str) -> Result<(), ServerError> {
        self.state.revoke(device_id).map_err(ServerError::State)?;
        self.logs.push(logs::LogEntry {
            id: 0,
            at_unix: auth::now_unix(),
            event: "revoked".into(),
            route: Some("/devices".into()),
            name: None,
            count: None,
            duration_ms: None,
            error: None,
        });
        Ok(())
    }
    /// `Some` sets (argon2id hash), `None` clears (`server.viewer-password`).
    pub fn set_viewer_password(&self, password: Option<&str>) -> Result<(), ServerError> {
        self.state.set_viewer_password(password).map_err(ServerError::State)
    }
    pub fn viewer_password_set(&self) -> bool {
        self.state.viewer_password_set()
    }
    /// Phones must pair: pairing required by the settings and no viewer password configured.
    pub fn pairing_required(&self) -> bool {
        self.cfg.require_pairing && !self.state.viewer_password_set()
    }
    pub fn logs(&self, after: u64) -> (Vec<logs::LogEntry>, u64) {
        self.logs.since(after)
    }
    pub async fn clear_cache(&self) {
        self.lookups.clear();
        self.proxy.cache.clear();
        proxy::clear_cache(&self.cfg.dir).await;
    }
    /// The config the server runs with (`port` = the bound one).
    pub fn config(&self) -> &ServerConfig {
        &self.cfg
    }
    /// `(cached objects, bytes)` of the media cache.
    pub fn cache_stats(&self) -> (u32, u64) {
        self.proxy.cache.stats()
    }
    /// `http://127.0.0.1:<port>/api/server/v1/media/`.
    pub fn local_media_base(&self) -> String {
        format!("http://127.0.0.1:{}{API_PREFIX}/media/", self.addr.port())
    }
    /// Loopback proxy URL of `url` + `headers` for this PC's player (`server.cors-proxy-local`): only a loopback peer
    /// may use the ticket ([`proxy::LOCAL_SESSION`]); HLS playlists are rewritten so segments and keys go through
    /// the proxy with per-origin headers.
    pub fn local_media_url(&self, url: &str, headers: &[(String, String)]) -> String {
        self.proxy.mint(&format!("http://127.0.0.1:{}", self.addr.port()), url, headers, proxy::LOCAL_SESSION, None)
    }
    /// `http://<lan ip>:<port>` URLs phones can use (empty while listening on loopback only).
    pub fn lan_urls(&self) -> Vec<String> {
        if self.cfg.loopback_only {
            return Vec::new();
        }
        let ips: Vec<Ipv4Addr> = match self.addr.ip() {
            IpAddr::V4(ip) if !ip.is_unspecified() => vec![ip],
            _ => lan_ips(),
        };
        urls_for(&ips, self.addr.port())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub addr: SocketAddr,
    pub started_unix: i64,
    pub server_id: Option<String>,
    pub devices: u32,
}

/// Bind `0.0.0.0:<port>` (`127.0.0.1` with [`ServerConfig::loopback_only`]; port `0` = ephemeral) and serve the
/// router of [`routes::router`] until [`ServerHandle::stop`].
pub async fn start(cfg: ServerConfig, ext: Arc<ExtensionHost>) -> Result<ServerHandle, ServerError> {
    let ip = if cfg.loopback_only { Ipv4Addr::LOCALHOST } else { Ipv4Addr::UNSPECIFIED };
    let wanted = SocketAddr::from((ip, cfg.port));
    let listener = tokio::net::TcpListener::bind(wanted).await.map_err(|source| ServerError::Bind { addr: wanted, source })?;
    let addr = listener.local_addr().map_err(|source| ServerError::Bind { addr: wanted, source })?;
    let state = Arc::new(state::ServerState::open(&cfg.dir)?);
    let logs = Arc::new(logs::LogBuffer::new());
    let upstream = upstream::Upstream::new(cfg.outbound_proxy.clone(), cfg.flaresolverr_url.clone(), cfg.timeout);
    let proxy = Arc::new(proxy::MediaProxy::new(&cfg.dir, upstream));
    let lookups = Arc::new(routes::LookupCache::default());
    let started_unix = auth::now_unix();
    let mut run_cfg = cfg;
    run_cfg.port = addr.port();
    let app = routes::router(routes::AppState {
        cfg: Arc::new(run_cfg.clone()),
        ext: ext.clone(),
        state: state.clone(),
        logs: logs.clone(),
        proxy: proxy.clone(),
        lookups: lookups.clone(),
        started_unix,
    });
    let shutdown = tokio_util::sync::CancellationToken::new();
    let stop = shutdown.clone();
    let task = tokio::spawn(async move {
        let svc = app.into_make_service_with_connect_info::<SocketAddr>();
        if let Err(e) = axum::serve(listener, svc).with_graceful_shutdown(async move { stop.cancelled().await }).await {
            tracing::warn!(error = %e, "extension server stopped with an error");
        }
    });
    logs.push(logs::LogEntry {
        id: 0,
        at_unix: started_unix,
        event: "started".into(),
        route: None,
        name: Some(addr.to_string()),
        count: None,
        duration_ms: None,
        error: None,
    });
    tracing::info!(%addr, "extension server listening");
    Ok(ServerHandle {
        cfg: run_cfg,
        ext,
        state,
        logs,
        proxy,
        lookups,
        shutdown,
        task: std::sync::Mutex::new(Some(task)),
        addr,
        started_unix,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_list_lan_addresses_then_loopback() {
        let ips = [Ipv4Addr::new(192, 168, 1, 10), Ipv4Addr::LOCALHOST, Ipv4Addr::UNSPECIFIED];
        assert_eq!(urls_for(&ips, 8099), vec!["http://192.168.1.10:8099".to_owned(), "http://127.0.0.1:8099".to_owned()]);
        assert!(!host_name().is_empty());
    }
}
