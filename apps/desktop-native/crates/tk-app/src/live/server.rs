//! Native extension server, media proxy and FlareSolverr (wave B, worker **k**; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `server.*`, `server.cors-proxy-local`). The server is `crates/tk-server` (axum,
//! `/api/server/v1`, docs/redesign/13 section 3.2) hosted in-process over the same `tk_ext::ExtensionHost` the app uses
//! (`b.ext.host()`): phones get the PC's `.kai` extensions. State (server id, device tokens, viewer password hash) in
//! `<data>/server/state.json` (`tk_server::state`); prefs in `Settings::server`.
//!
//! * `on_start` / `on_settings` keep the LAN server in line with `Settings::server` (`enabled`, port, name, pairing,
//!   proxy, timeouts, FlareSolverr, outbound proxy): start, stop or restart when a field the server reads changed.
//! * The loopback media proxy (`server.cors-proxy-local`): [`local_stream`] rewrites a stream whose headers mpv cannot
//!   apply safely to `http://127.0.0.1:<port>/api/server/v1/media/<ticket>/...` (HLS playlists rewritten, segments
//!   and keys proxied with per-origin headers, `Range` passthrough). It uses the LAN server when it runs, else a
//!   private instance bound to 127.0.0.1 on an ephemeral port, started on demand.
//! * Extension fetches (`server.proxy`, `server.flaresolverr`): the same outbound proxy and FlareSolverr are installed
//!   as the route of `tk-ext`'s `SafeHttp` ([`apply_extension_route`] -> `tk_core::net::set_extension_outbound`), so
//!   extension HTTP goes through the proxy and a Cloudflare challenge is solved once and retried. It does not depend
//!   on the LAN server being on.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tk_server::{ServerConfig, ServerError, ServerHandle};
use tk_ui::backend::{
    FlareSolverrVm, ProxyStatusVm, ServerDeviceVm, ServerLogPageVm, ServerLogVm, ServerPairingVm, ServerStatusVm, StreamVm,
    UiResult,
};
use tk_ui::model::server::{PROXY_TEST_PREFIX, ServerPrefs, note_stream_kind};
use tk_ui::model::{ErrorKind, Settings, UiError};

use crate::live::LiveBackend;
use crate::paths::AppPaths;
use crate::settings_store;

/// How long "Test" waits for FlareSolverr / the outbound proxy.
const TEST_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Default)]
struct Running {
    /// The LAN server (`Settings::server.enabled`).
    lan: Option<Arc<ServerHandle>>,
    /// The prefs `lan` was started with.
    lan_prefs: Option<ServerPrefs>,
    /// Why the last start failed (shown by `status` until the next attempt).
    error: Option<UiError>,
    /// Loopback-only instance for mpv while the LAN server is off, and the prefs it was started with.
    local: Option<(Arc<ServerHandle>, ServerPrefs)>,
    /// `(outbound proxy, FlareSolverr)` installed for extension fetches (`None` = never installed).
    route: Option<RouteKey>,
}

/// What the extension route is built from.
type RouteKey = (Option<String>, Option<String>);

/// The extension route `prefs` asks for (pure).
pub fn route_key(p: &ServerPrefs) -> RouteKey {
    (p.outbound_proxy.clone(), p.flaresolverr_url.clone())
}

/// Install the outbound route of `prefs` for extension fetches (`server.proxy`, `server.flaresolverr`), only when it
/// changed: the cached Cloudflare clearances survive unrelated saves.
fn apply_extension_route(run: &mut Running, prefs: &ServerPrefs) {
    let key = route_key(prefs);
    if run.route.as_ref() == Some(&key) {
        return;
    }
    let route = tk_server::upstream::extension_route(key.0.clone(), key.1.clone())
        .map(|r| r as Arc<dyn tk_core::net::Outbound>);
    tk_core::net::set_extension_outbound(route);
    run.route = Some(key);
}

/// The in-process server handles and their config.
pub struct ExtServer {
    paths: AppPaths,
    #[allow(dead_code)]
    client: reqwest::Client,
    running: tokio::sync::Mutex<Running>,
}

impl ExtServer {
    pub fn new(paths: AppPaths, client: reqwest::Client) -> Self {
        Self { paths, client, running: tokio::sync::Mutex::new(Running::default()) }
    }

    fn dir(&self) -> PathBuf {
        self.paths.root.join("server")
    }

    /// The loopback-only instance keeps its own directory (its media cache is wiped at start).
    fn local_dir(&self) -> PathBuf {
        self.paths.root.join("server-local")
    }
}

/// The server config of `prefs` (pure).
pub fn config_for(dir: PathBuf, p: &ServerPrefs) -> ServerConfig {
    let mut c = ServerConfig::new(dir, p.port);
    c.name = p.name.clone();
    c.require_pairing = p.require_pairing;
    c.proxy_enabled = p.proxy_enabled;
    c.allowed_origins = p.allowed_origins.clone();
    c.timeout = Duration::from_secs(u64::from(p.timeout_secs));
    c.concurrency = usize::from(p.concurrency);
    c.flaresolverr_url = p.flaresolverr_url.clone();
    c.outbound_proxy = p.outbound_proxy.clone();
    c
}

/// Whether the running LAN server must restart for `next` (a field it reads changed). Pure.
pub fn needs_restart(running: &ServerPrefs, next: &ServerPrefs) -> bool {
    running.port != next.port
        || running.name != next.name
        || running.require_pairing != next.require_pairing
        || running.proxy_enabled != next.proxy_enabled
        || running.allowed_origins != next.allowed_origins
        || running.timeout_secs != next.timeout_secs
        || running.concurrency != next.concurrency
        || running.flaresolverr_url != next.flaresolverr_url
        || running.outbound_proxy != next.outbound_proxy
}

/// Whether mpv should get `headers` through the loopback proxy instead of `--http-header-fields`: mpv sends its
/// header fields to EVERY request of the stream (segments on other CDNs, key servers), so credentials (`Cookie`,
/// `Authorization`) and `Origin` would leak across origins or break CORS-checked CDNs; and with an outbound proxy
/// configured every stream goes through it. Pure.
pub fn needs_local_proxy(headers: &[(String, String)], outbound_proxy: bool) -> bool {
    outbound_proxy || headers.iter().any(|(k, _)| matches!(k.to_ascii_lowercase().as_str(), "cookie" | "authorization" | "origin"))
}

fn bind_err(e: &ServerError, port: u16) -> UiError {
    match e {
        ServerError::Bind { source, .. }
            if source.kind() == std::io::ErrorKind::AddrInUse || matches!(source.raw_os_error(), Some(10048 | 98)) =>
        {
            UiError::new(ErrorKind::AddrInUse, format!("io: {source} · bind :{port}"))
        }
        other => UiError::new(ErrorKind::Other, other.to_string()),
    }
}

fn not_running() -> UiError {
    UiError::new(ErrorKind::Other, "the extension server is not running")
}

fn state_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn prefs_of(b: &LiveBackend) -> ServerPrefs {
    settings_store::load(&b.settings_file).server.sanitized()
}

async fn start_lan(b: &LiveBackend, run: &mut Running, prefs: &ServerPrefs) -> UiResult<Arc<ServerHandle>> {
    if let Some(h) = run.lan.take() {
        h.stop().await;
    }
    run.lan_prefs = None;
    match tk_server::start(config_for(b.server.dir(), prefs), b.ext.host()).await {
        Ok(h) => {
            let h = Arc::new(h);
            run.lan = Some(h.clone());
            run.lan_prefs = Some(prefs.clone());
            run.error = None;
            // the LAN server now serves mpv too: the loopback instance is not needed any more
            if let Some((local, _)) = run.local.take() {
                local.stop().await;
            }
            Ok(h)
        }
        Err(e) => {
            tracing::warn!(port = prefs.port, error = %e, "extension server did not start");
            let ui = bind_err(&e, prefs.port);
            run.error = Some(ui.clone());
            Err(ui)
        }
    }
}

async fn stop_lan(run: &mut Running) {
    if let Some(h) = run.lan.take() {
        h.stop().await;
    }
    run.lan_prefs = None;
}

/// Start-up: install the extension route, start `tk-server` when `Settings::server.enabled` (`server.native-mode`)
/// and apply the mDNS switch of Link (`link.discovery`). A failed start is only logged (the Server page shows it).
pub async fn on_start(b: &LiveBackend) {
    let prefs = prefs_of(b);
    super::link::set_announce(b, prefs.link_announce).await;
    let mut run = b.server.running.lock().await;
    apply_extension_route(&mut run, &prefs);
    if prefs.enabled {
        let _ = start_lan(b, &mut run, &prefs).await;
    }
}

/// `Settings::server` changed: re-install the extension route, start / stop / restart the LAN server, drop a
/// loopback instance started with other proxy settings, and apply the mDNS switch.
pub async fn on_settings(b: &LiveBackend, settings: &Settings) {
    let prefs = settings.server.clone().sanitized();
    super::link::set_announce(b, prefs.link_announce).await;
    let mut run = b.server.running.lock().await;
    apply_extension_route(&mut run, &prefs);
    if let Some((local, used)) = run.local.as_ref()
        && (used.outbound_proxy != prefs.outbound_proxy || used.flaresolverr_url != prefs.flaresolverr_url)
    {
        local.stop().await;
        run.local = None;
    }
    match (prefs.enabled, run.lan_prefs.clone()) {
        (true, Some(cur)) if !needs_restart(&cur, &prefs) => run.lan_prefs = Some(prefs),
        (true, _) => {
            let _ = start_lan(b, &mut run, &prefs).await;
        }
        (false, Some(_)) => stop_lan(&mut run).await,
        (false, None) => run.error = None,
    }
}

fn enabled_sources(b: &LiveBackend) -> u32 {
    b.ext.installed().into_iter().filter(|e| e.enabled && e.compat != tk_core::Compat::Unsupported).count() as u32
}

fn display_name(prefs: &ServerPrefs) -> String {
    if prefs.name.trim().is_empty() { tk_server::host_name() } else { prefs.name.trim().to_owned() }
}

fn status_vm(b: &LiveBackend, run: &Running) -> ServerStatusVm {
    let prefs = prefs_of(b);
    match &run.lan {
        Some(h) => {
            let st = h.status();
            ServerStatusVm {
                running: true,
                port: st.addr.port(),
                urls: h.lan_urls(),
                name: display_name(run.lan_prefs.as_ref().unwrap_or(&prefs)),
                server_id: st.server_id,
                devices: st.devices,
                pairing_required: h.pairing_required(),
                viewer_password_set: h.viewer_password_set(),
                sources: enabled_sources(b),
                started_unix: Some(st.started_unix),
                error: None,
            }
        }
        None => {
            // stopped: what the saved state knows (ids, devices, password) without listening
            let state = tk_server::state::ServerState::open(&b.server.dir()).ok();
            let password = state.as_ref().is_some_and(|s| s.viewer_password_set());
            ServerStatusVm {
                running: false,
                port: prefs.port,
                urls: Vec::new(),
                name: display_name(&prefs),
                server_id: state.as_ref().and_then(|s| s.server_id()),
                devices: state.as_ref().map_or(0, |s| s.device_count()),
                pairing_required: prefs.require_pairing && !password,
                viewer_password_set: password,
                sources: enabled_sources(b),
                started_unix: None,
                error: run.error.clone(),
            }
        }
    }
}

pub async fn status(b: &LiveBackend) -> UiResult<ServerStatusVm> {
    let run = b.server.running.lock().await;
    Ok(status_vm(b, &run))
}

/// Start with the saved prefs (the page also saves `enabled = true`, so the next launch starts it too). A taken
/// port is reported in `ServerStatusVm::error`, not as an `Err`.
pub async fn start(b: &LiveBackend) -> UiResult<ServerStatusVm> {
    let prefs = prefs_of(b);
    let mut run = b.server.running.lock().await;
    if run.lan.is_none() || run.lan_prefs.as_ref().is_some_and(|cur| needs_restart(cur, &prefs)) {
        let _ = start_lan(b, &mut run, &prefs).await;
    }
    Ok(status_vm(b, &run))
}

pub async fn stop(b: &LiveBackend) -> UiResult<()> {
    let mut run = b.server.running.lock().await;
    stop_lan(&mut run).await;
    run.error = None;
    Ok(())
}

async fn lan(b: &LiveBackend) -> UiResult<Arc<ServerHandle>> {
    b.server.running.lock().await.lan.clone().ok_or_else(not_running)
}

pub async fn open_pairing(b: &LiveBackend) -> UiResult<ServerPairingVm> {
    let h = lan(b).await?;
    let origin = h.lan_urls().into_iter().next().unwrap_or_else(|| format!("http://127.0.0.1:{}", h.addr.port()));
    let p = h.create_pairing(&origin).map_err(state_err)?;
    Ok(ServerPairingVm {
        qr_svg: tk_ui::qr::qr_svg(&p.deep_link).unwrap_or_default(),
        code: p.code,
        expires_in_secs: p.expires_in,
        deep_link: p.deep_link,
    })
}

fn device_vm(d: tk_server::state::DeviceRecord) -> ServerDeviceVm {
    ServerDeviceVm { id: d.id, label: d.label, role: d.role, created_unix: d.created_at, last_seen_unix: d.last_seen_at, current: false }
}

pub async fn devices(b: &LiveBackend) -> UiResult<Vec<ServerDeviceVm>> {
    let list = match lan(b).await {
        Ok(h) => h.devices(),
        Err(_) => tk_server::state::ServerState::open(&b.server.dir()).map_err(state_err)?.devices(),
    };
    let mut rows: Vec<ServerDeviceVm> = list.into_iter().map(device_vm).collect();
    rows.sort_by(|a, b| b.last_seen_unix.cmp(&a.last_seen_unix).then_with(|| a.label.cmp(&b.label)));
    Ok(rows)
}

/// Revoking works with the server stopped too (the token is removed from `state.json`); a running server denies
/// the token on its next request (`401 device_not_authorized`).
pub async fn revoke(b: &LiveBackend, device_id: &str) -> UiResult<()> {
    match lan(b).await {
        Ok(h) => h.revoke(device_id).map_err(state_err),
        Err(_) => tk_server::state::ServerState::open(&b.server.dir()).and_then(|s| s.revoke(device_id)).map_err(state_err),
    }
}

/// Argon2id hash in `state.json` (never in `ui-settings.json`); 10..=1024 characters.
pub async fn set_viewer_password(b: &LiveBackend, password: Option<&str>) -> UiResult<()> {
    let password = password.map(str::to_owned).filter(|p| !p.is_empty());
    let r = match lan(b).await {
        Ok(h) => tokio::task::spawn_blocking(move || h.set_viewer_password(password.as_deref()).map_err(|e| e.to_string())).await,
        Err(_) => {
            let dir = b.server.dir();
            tokio::task::spawn_blocking(move || {
                tk_server::state::ServerState::open(&dir).and_then(|s| s.set_viewer_password(password.as_deref())).map_err(|e| e.to_string())
            })
            .await
        }
    };
    r.map_err(state_err)?.map_err(|e| UiError::new(ErrorKind::Other, e))
}

fn log_vm(e: tk_server::logs::LogEntry) -> ServerLogVm {
    ServerLogVm { id: e.id, at_unix: e.at_unix, event: e.event, route: e.route, name: e.name, count: e.count, duration_ms: e.duration_ms, error: e.error }
}

/// Lines newer than `after`. A restarted server starts a new log (its cursor is lower than `after`): then the
/// whole new log is returned and the page starts over.
pub async fn logs(b: &LiveBackend, after: u64) -> UiResult<ServerLogPageVm> {
    let Ok(h) = lan(b).await else { return Ok(ServerLogPageVm { entries: Vec::new(), cursor: after }) };
    let (mut entries, cursor) = h.logs(after);
    if cursor < after {
        entries = h.logs(0).0;
    }
    Ok(ServerLogPageVm { entries: entries.into_iter().map(log_vm).collect(), cursor })
}

pub async fn proxy_status(b: &LiveBackend) -> UiResult<ProxyStatusVm> {
    let prefs = prefs_of(b);
    let run = b.server.running.lock().await;
    let handle = run.lan.clone().or_else(|| run.local.as_ref().map(|(h, _)| h.clone()));
    let (cache_entries, cache_bytes) = handle.as_ref().map_or((0, 0), |h| h.cache_stats());
    Ok(ProxyStatusVm {
        enabled: prefs.proxy_enabled,
        base_url: handle.as_ref().map(|h| h.local_media_base()),
        cache_entries,
        cache_bytes,
        max_cache_bytes: tk_server::proxy::MAX_CACHE_BYTES,
    })
}

pub async fn clear_cache(b: &LiveBackend) -> UiResult<()> {
    let run = b.server.running.lock().await;
    let handles: Vec<Arc<ServerHandle>> = run.lan.iter().cloned().chain(run.local.iter().map(|(h, _)| h.clone())).collect();
    drop(run);
    if handles.is_empty() {
        tk_server::proxy::clear_cache(&b.server.dir()).await;
    }
    for h in handles {
        h.clear_cache().await;
    }
    Ok(())
}

/// "Test" of Settings > Server & network. `url` = the FlareSolverr endpoint to test (`None` = the saved one); with
/// the [`PROXY_TEST_PREFIX`] it is an outbound proxy URL instead (`server.proxy`).
pub async fn flaresolverr_test(b: &LiveBackend, url: Option<&str>) -> UiResult<FlareSolverrVm> {
    if let Some(proxy) = url.and_then(|u| u.strip_prefix(PROXY_TEST_PREFIX)) {
        return Ok(match tk_server::upstream::probe_proxy(proxy.trim(), TEST_TIMEOUT).await {
            Ok(latency) => FlareSolverrVm { ok: true, version: None, latency_ms: Some(latency), error: None },
            Err(e) => FlareSolverrVm { ok: false, version: None, latency_ms: None, error: Some(e) },
        });
    }
    let endpoint = url.map(str::to_owned).filter(|u| !u.trim().is_empty()).or_else(|| prefs_of(b).flaresolverr_url);
    let Some(endpoint) = endpoint else {
        return Ok(FlareSolverrVm { ok: false, version: None, latency_ms: None, error: Some("no FlareSolverr address configured".into()) });
    };
    Ok(match tk_server::upstream::probe_flaresolverr(endpoint.trim().trim_end_matches('/'), TEST_TIMEOUT).await {
        Ok((version, latency)) => FlareSolverrVm { ok: true, version, latency_ms: Some(latency), error: None },
        Err(e) => FlareSolverrVm { ok: false, version: None, latency_ms: None, error: Some(e) },
    })
}

/// The handle that serves mpv's loopback proxy: the LAN server when it runs with its media proxy on, else the
/// loopback-only instance (started on demand with the current prefs).
async fn local_handle(b: &LiveBackend, prefs: &ServerPrefs) -> Option<Arc<ServerHandle>> {
    let mut run = b.server.running.lock().await;
    if let (Some(h), Some(p)) = (&run.lan, &run.lan_prefs)
        && p.proxy_enabled
    {
        return Some(h.clone());
    }
    if let Some((h, _)) = &run.local {
        return Some(h.clone());
    }
    let mut cfg = config_for(b.server.local_dir(), prefs);
    cfg.port = 0;
    cfg.loopback_only = true;
    cfg.proxy_enabled = true;
    cfg.require_pairing = true;
    match tk_server::start(cfg, b.ext.host()).await {
        Ok(h) => {
            let h = Arc::new(h);
            run.local = Some((h.clone(), prefs.clone()));
            Some(h)
        }
        Err(e) => {
            tracing::warn!(error = %e, "loopback media proxy did not start; playing the stream directly");
            None
        }
    }
}

/// `server.cors-proxy-local`: `stream` as mpv should open it. When its headers need the proxy
/// ([`needs_local_proxy`]) and `Settings::server.local_proxy` is on, the URL becomes a loopback ticket of tk-server
/// (the proxy sends the headers upstream, per origin) and mpv gets no header fields; otherwise it is unchanged.
/// Never fails: without a proxy the stream is played directly.
pub async fn local_stream(b: &LiveBackend, mut stream: StreamVm) -> StreamVm {
    let prefs = prefs_of(b);
    if !prefs.local_proxy || !needs_local_proxy(&stream.headers, prefs.outbound_proxy.is_some()) {
        return stream;
    }
    if !(stream.url.starts_with("http://") || stream.url.starts_with("https://")) {
        return stream;
    }
    let Some(h) = local_handle(b, &prefs).await else { return stream };
    stream.url = h.local_media_url(&stream.url, &stream.headers);
    stream.headers = Vec::new();
    note_stream_kind(&stream.url, "proxy");
    stream
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_follows_the_prefs() {
        let p = ServerPrefs { port: 9001, name: "PC".into(), timeout_secs: 30, concurrency: 2, ..ServerPrefs::default() };
        let c = config_for(PathBuf::from("/tmp/x"), &p);
        assert_eq!((c.port, c.name.as_str(), c.timeout, c.concurrency), (9001, "PC", Duration::from_secs(30), 2));
        assert!(c.require_pairing && c.proxy_enabled && !c.loopback_only);
    }

    #[test]
    fn restart_only_when_the_server_reads_the_change() {
        let a = ServerPrefs::default();
        assert!(!needs_restart(&a, &ServerPrefs { sniffer_timeout_secs: 30, enabled: true, ..a.clone() }));
        assert!(needs_restart(&a, &ServerPrefs { port: 9000, ..a.clone() }));
        assert!(needs_restart(&a, &ServerPrefs { outbound_proxy: Some("socks5://h:1".into()), ..a.clone() }));
    }

    #[test]
    fn extension_route_follows_proxy_and_flaresolverr_only() {
        let a = ServerPrefs::default();
        assert_eq!(route_key(&a), (None, None));
        let b = ServerPrefs { port: 9000, sniffer_timeout_secs: 30, ..a.clone() };
        assert_eq!(route_key(&a), route_key(&b), "unrelated fields keep the installed route (and its clearances)");
        let c = ServerPrefs { outbound_proxy: Some("socks5h://127.0.0.1:9050".into()), ..a.clone() };
        assert_ne!(route_key(&a), route_key(&c));
        let d = ServerPrefs { flaresolverr_url: Some("http://localhost:8191".into()), ..a.clone() };
        assert_eq!(route_key(&d), (None, Some("http://localhost:8191".to_owned())));
    }

    #[test]
    fn local_proxy_rule() {
        let h = |k: &str| vec![(k.to_owned(), "v".to_owned())];
        assert!(!needs_local_proxy(&h("Referer"), false));
        assert!(!needs_local_proxy(&h("User-Agent"), false));
        assert!(needs_local_proxy(&h("Cookie"), false));
        assert!(needs_local_proxy(&h("origin"), false));
        assert!(needs_local_proxy(&[], true), "an outbound proxy routes every stream");
    }
}
