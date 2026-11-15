//! tk-link: Tatakai Link v1 server (PC side). See `docs/redesign/03-pairing-upscale.md` and
//! `contracts/link/openapi.v1.json`.
//!
//! * identity: own CA + server certificate (rcgen/ring), keys protected with DPAPI
//! * pairing: 5-minute single-use window, <= 5 attempts, Crockford code, `tatakai-link:v1?...` payload
//! * transport: rustls (ring) + optional client certs, axum routes, per-request device authz, hot revocation
//! * `url` source: SSRF-safe loopback fetcher + HLS rewriting; FFmpeg only ever reads the loopback
//! * sessions: on-demand fMP4 HLS, 512 MiB RAM window, heartbeat/idle, FFmpeg in a Windows Job Object (Linux:
//!   `PR_SET_PDEATHSIG`)
//! * media (ported from Enigmata-Temple's PC pipeline): Windows D3D11 + AMF/NVENC/QSV, Linux VAAPI decode/encode with
//!   libx264/libopenh264 fallback, Anime4K via libplacebo, experimental LSFG frame generation through the
//!   `temple-lsfg` helper (FFmpeg -> temple-lsfg -> FFmpeg), see [`media`]
//!
//! Wave B (worker k) adds `POST /v1/control`, `GET /v1/player`, the `GET /v1/events` long-poll (send to device /
//! handoff) and the mDNS announcer (`discovery`, `_tatakai-link._tcp`, TXT v / name / id / fp = CA fingerprint).
//! Still out by design: TOFU without pairing, `/v1/settings`, downloads, the phone source.
//!
//! Wave A (plan 14 section 0.1 #9, worker **f**): `GET` / `POST /v1/history` sync the watch history (exact resume
//! position) between the user's own devices, replacing the removed cloud history. See [`history`].

pub mod api;
/// Wave B (worker k): `POST /v1/control` wire form (remote control / hand-off from a paired phone).
pub mod control;
pub mod devices;
/// Wave B (worker k): mDNS announcement `_tatakai-link._tcp` (`link.discovery`).
pub mod discovery;
pub mod fetcher;
pub mod firewall;
pub mod history;
pub mod identity;
pub mod library;
pub mod media;
pub mod pairing;
pub mod playlist;
pub mod protect;
pub mod server;
pub mod sessions;
mod sink;

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use time::OffsetDateTime;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast};
use tokio_util::sync::CancellationToken;

pub use devices::Device;
pub use library::LibraryItem;
pub use media::capabilities::{Capabilities, CommandRunner, GpuInfo, Vendor};
pub use media::engine::Engine;
pub use media::lsfg::{LsfgCapability, LsfgStatus};
pub use pairing::{PairingPayload, WindowStatus};
pub use sessions::SessionTuning;

use crate::identity::Identity;
use crate::media::capabilities::{BenchStore, Environment, SystemRunner, detect, guess_gpu_with};
use crate::media::engine::{EngineJob, EngineProcess, FfmpegEngine};
use crate::pairing::{DEFAULT_MAX_ATTEMPTS, DEFAULT_TTL, PairingWindow};
use crate::server::AppState;
use tk_core::net::{Policy, SafeHttp};

pub const DEFAULT_PORT: u16 = 47321;

/// BUG-039: a device seen this recently counts as connected (two long-poll rounds of `GET /v1/events` + slack).
pub const ONLINE_WINDOW_SECS: i64 = 2 * control::EVENTS_MAX_WAIT_SECS as i64 + 15;

/// Devices with a session plus the active ones seen within [`ONLINE_WINDOW_SECS`] of `now`. Pure.
pub fn online_ids(with_session: Vec<String>, devices: &[Device], now: i64) -> Vec<String> {
    let mut out = with_session;
    for d in devices {
        let recent = d.last_seen_at.is_some_and(|t| now.saturating_sub(t) <= ONLINE_WINDOW_SECS && t <= now + 5);
        if recent && d.is_active(now) && !out.contains(&d.id) {
            out.push(d.id.clone());
        }
    }
    out
}

/// What the host application provides: the optional user library folder, the FFmpeg location and (for the
/// extension-permission intersection of the `url` source) the permissions of installed extensions.
pub trait LinkHost: Send + Sync + 'static {
    fn library(&self) -> Vec<LibraryItem>;
    fn library_path(&self, id: &str) -> Option<PathBuf>;
    fn ffmpeg_path(&self) -> Option<PathBuf>;
    /// `network:domain:*` permissions of the installed extension `id`; `None` when it is not installed.
    fn extension_permissions(&self, _id: &str) -> Option<Vec<String>> {
        None
    }
    /// Watch-history rows with `watched_at > since_unix` (unix seconds; `0` = all), OLDEST first, at most
    /// `limit` (the route asks `limit + 1` so [`history::page`] can flag truncation). Blocking (SQLite): the route
    /// calls it from `spawn_blocking`. Default: no history (hosts without a store).
    fn history_since(&self, _since_unix: i64, _limit: usize) -> Vec<tk_core::HistoryEntry> {
        Vec::new()
    }
    /// Merge rows pushed by a phone into the local history (newest `watched_at` wins); returns how many were
    /// stored. Blocking. Default: nothing stored.
    fn history_merge(&self, _rows: Vec<tk_core::HistoryEntry>) -> usize {
        0
    }
    /// 2026-11 history per device: rows pushed by `device_id` (the authenticated phone). The host remembers the device
    /// as the rows' origin; with `authoritative` (the device is the chosen source and its window is open) every row
    /// wins regardless of timestamps. Returns how many were stored. Default: [`history_merge`](Self::history_merge).
    fn history_merge_from(&self, _device_id: &str, rows: Vec<tk_core::HistoryEntry>, _authoritative: bool) -> usize {
        self.history_merge(rows)
    }
    /// Drop the whole local history (`DELETE /v1/history`, user-confirmed on the phone). Returns `false` when the
    /// host cannot (no store): the route then answers `internal`. Default: `false`.
    fn history_clear(&self) -> bool {
        false
    }
    /// Wave B (worker k; checklist `link.remote-control`, `link.handoff`): a paired phone drives this PC's player
    /// (`POST /v1/control`, see [`control`]). Returns `false` when no player can take it (the route answers `409
    /// no_player`). Default: not accepted.
    fn control(&self, _cmd: control::ControlCommand) -> bool {
        false
    }
    /// Wave B (worker k; `link.remote-control`): what the player shows now, for `GET /v1/player`; `None` = no player
    /// open (the route answers `404 no_player`). Default: `None`.
    fn player_state(&self) -> Option<control::PlayerState> {
        None
    }
    /// BUG-023: exact local endpoints a `url` source may reach although they are private addresses, i.e. this PC's
    /// own extension-server media proxy (`http://<lan ip>:<port>/api/server/v1/media/`). Phones linked to that
    /// server receive its URLs as sources; they already reach them directly, so letting the PC read them opens
    /// nothing new. Default: none.
    fn trusted_media_origins(&self) -> Vec<tk_core::net::TrustedOrigin> {
        Vec::new()
    }
    /// BUG-009: add / reuse the torrent of a `torrent` session source in the PC's engine, pick the episode's file and
    /// return its loopback stream (the engine's HTTP server, Range-capable). Default: no torrent engine
    /// (`source_unavailable`).
    fn open_torrent(&self, _req: TorrentRequest) -> HostFuture<'_, Result<OpenedTorrent, api::ErrorCode>> {
        Box::pin(async { Err(api::ErrorCode::SourceUnavailable) })
    }
    /// BUG-131: a renewed session (profile change, reopen) reads the torrent `torrent_id` an earlier session opened.
    /// `true` = the engine still has it and it transfers (resumed if it was paused); the host counts it as one more
    /// reader, exactly like an [`open_torrent`](Self::open_torrent), and a matching
    /// [`torrent_session_ended`](Self::torrent_session_ended) follows. `false` = gone: the session opens it again.
    /// Default: `false`.
    fn reuse_torrent(&self, _torrent_id: &str) -> HostFuture<'_, bool> {
        Box::pin(async { false })
    }
    /// BUG-009: a Link session reading `torrent_id` ended (one call per successful
    /// [`open_torrent`](Self::open_torrent) / [`reuse_torrent`](Self::reuse_torrent)). BUG-131: a renew opens the new
    /// session before it closes the old one, so a host that counts readers only runs its after-watch policy when the
    /// last one ends. Default: nothing.
    fn torrent_session_ended(&self, _torrent_id: &str) {}
}

/// Boxed future of the async [`LinkHost`] calls (no extra dependency for hosts).
pub type HostFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// What a `torrent` session source asks the host for (BUG-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentRequest {
    /// Magnet URI, 40-hex info-hash or a PC source id of a torrent candidate.
    pub uri: String,
    /// Title id on the PC (AniList id), for the episode's file in a pack.
    pub media_id: String,
    pub episode: u32,
}

/// The loopback stream of an opened torrent: `http://127.0.0.1:<port>/<token>/<hash>/<index>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedTorrent {
    pub url: String,
    pub torrent_id: String,
}

/// The exact origin of the engine's stream server for `url` (loopback literal + port + `/<token>/`), trusted for that
/// session only. `None` for anything else. Pure.
pub fn torrent_stream_origin(url: &url::Url) -> Option<tk_core::net::TrustedOrigin> {
    let ip = match url.host()? {
        url::Host::Ipv4(v) if v.is_loopback() => IpAddr::V4(v),
        _ => return None,
    };
    let token = url.path_segments()?.next().filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))?;
    Some(tk_core::net::TrustedOrigin::new(SocketAddr::new(ip, url.port_or_known_default()?), format!("/{token}/")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkEvent {
    DevicePaired { device_id: String, name: String },
    DeviceRevoked { device_id: String },
    SessionStarted { device_id: String, session_id: String },
    SessionEnded { session_id: String },
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("identity: {0}")]
    Identity(#[from] identity::IdentityError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    #[error("refusing to bind {0}: only private LAN or loopback addresses are allowed")]
    PublicBind(IpAddr),
    #[error("no LAN address found to listen on")]
    NoAddress,
}

/// Whether and how Link looks for LSFG (`temple-lsfg` + `Lossless.dll`).
#[derive(Debug, Clone, Default)]
pub enum LsfgSetup {
    /// No LSFG profiles (tests, hosts that do not opt in). Reported as `lsfg.reason = "disabled"`.
    #[default]
    Off,
    /// Find the helper (`TEMPLE_LSFG`, next to the executable or one folder above) and the DLL (`dll` when given,
    /// else Steam incl. `libraryfolders.vdf`), then run `temple-lsfg --probe` once at start.
    Detect { dll: Option<PathBuf> },
    /// A precomputed status (tests).
    Status(LsfgStatus),
}

pub struct LinkConfig {
    /// `%APPDATA%\Tatakai-Next\link` (resolved by the app; tests pass a tempdir).
    pub data_dir: PathBuf,
    /// 47321 by default; 0 picks an ephemeral port (tests).
    pub port: u16,
    /// Addresses to listen on. Empty = the detected LAN IPv4. Never 0.0.0.0, never public addresses.
    pub bind_ips: Vec<Ipv4Addr>,
    pub server_name: String,
    /// Directory with the 14 Anime4K `.glsl` files (downloaded by `packaging/fetch-deps.ps1`). `None` = no Anime4K profiles.
    pub anime4k_dir: Option<PathBuf>,
    /// `packaging/anime4k.lock.json`: when given, the shaders must match it or the Anime4K profiles are withheld.
    pub anime4k_lock: Option<PathBuf>,
    /// "Allow HTTP" setting for `url` sources (https only by default).
    pub allow_http_sources: bool,
    pub client_fg_allowed: bool,
    /// Real GPU info from the app (DXGI). `None` = guessed from the detected encoders (Linux: the VAAPI render
    /// node's PCI vendor; the name from the LSFG probe when it ran).
    pub gpu: Option<GpuInfo>,
    /// LSFG frame generation (experimental). Default: off.
    pub lsfg: LsfgSetup,
    pub tuning: SessionTuning,
    /// Test hooks (production leaves these `None`).
    pub engine: Option<Arc<dyn Engine>>,
    pub runner: Option<Arc<dyn CommandRunner>>,
    pub fetch_http: Option<SafeHttp>,
}

impl LinkConfig {
    pub fn new(data_dir: PathBuf, server_name: impl Into<String>) -> Self {
        Self {
            data_dir,
            port: DEFAULT_PORT,
            bind_ips: Vec::new(),
            server_name: server_name.into(),
            anime4k_dir: None,
            anime4k_lock: None,
            allow_http_sources: false,
            client_fg_allowed: false,
            gpu: None,
            lsfg: LsfgSetup::Off,
            tuning: SessionTuning::default(),
            engine: None,
            runner: None,
            fetch_http: None,
        }
    }
}

/// Primary private LAN IPv4 (UDP "connect" trick: no packet is sent).
pub fn detect_lan_ips() -> Vec<Ipv4Addr> {
    let Ok(sock) = std::net::UdpSocket::bind(("0.0.0.0", 0)) else { return Vec::new() };
    if sock.connect(("192.0.2.1", 9)).is_err() {
        return Vec::new();
    }
    match sock.local_addr() {
        Ok(SocketAddr::V4(a)) if is_private_or_loopback(*a.ip()) && !a.ip().is_loopback() => vec![*a.ip()],
        _ => Vec::new(),
    }
}

fn is_private_or_loopback(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

struct NoEngine;

#[async_trait::async_trait]
impl Engine for NoEngine {
    async fn spawn(&self, _job: EngineJob) -> std::io::Result<Box<dyn EngineProcess>> {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "FFmpeg is not installed"))
    }
}

pub struct LinkHandle {
    state: AppState,
    addrs: Vec<SocketAddr>,
    shutdown: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    sink_task: tokio::task::JoinHandle<()>,
    fetcher: Arc<fetcher::Fetcher>,
}

/// Start the Link server.
pub async fn start(cfg: LinkConfig, host: Arc<dyn LinkHost>) -> Result<LinkHandle, LinkError> {
    let mut bind_ips = cfg.bind_ips.clone();
    if bind_ips.is_empty() {
        bind_ips = detect_lan_ips();
    }
    if bind_ips.is_empty() {
        return Err(LinkError::NoAddress);
    }
    if let Some(bad) = bind_ips.iter().find(|ip| !is_private_or_loopback(**ip)) {
        return Err(LinkError::PublicBind(IpAddr::V4(*bad)));
    }

    // listeners first: a taken port (os error 10048 / 98) fails before any task, fetcher or sink is started,
    // so a failed start leaves nothing running and the app can retry or pick another port.
    let mut listeners = Vec::new();
    let mut port = cfg.port;
    for ip in &bind_ips {
        let l = TcpListener::bind((*ip, port)).await?;
        if port == 0 {
            port = l.local_addr()?.port();
        }
        listeners.push(l);
    }

    let now = OffsetDateTime::now_utc();
    let ips: Vec<IpAddr> = bind_ips.iter().map(|i| IpAddr::V4(*i)).collect();
    let identity = Arc::new(Identity::load_or_init(&cfg.data_dir, &ips, now)?);
    let devices = Arc::new(devices::DeviceRegistry::open(cfg.data_dir.join("devices.json"))?);
    let tls = server::tls_config(&identity)?;

    // environment: encoders / filters / GPU / shaders
    let ffmpeg = host.ffmpeg_path();
    let runner: Arc<dyn CommandRunner> = cfg.runner.clone().unwrap_or_else(|| Arc::new(SystemRunner));
    let detected = match &ffmpeg {
        Some(p) => detect(p, runner.as_ref()).await,
        None => Default::default(),
    };
    let shaders_ok = cfg.anime4k_dir.as_deref().is_some_and(|dir| {
        media::anime4k::shaders_present(dir)
            && match &cfg.anime4k_lock {
                Some(lock) => std::fs::read_to_string(lock)
                    .map_err(|e| e.to_string())
                    .and_then(|text| media::anime4k::verify_against_lock(dir, &text))
                    .inspect_err(|e| tracing::warn!(error = %e, "Anime4K shaders rejected"))
                    .is_ok(),
                None => true,
            }
    });
    let render_node = media::vaapi::discover_system();
    let lsfg = match (&cfg.lsfg, &ffmpeg) {
        (LsfgSetup::Off, _) => LsfgStatus::unavailable(media::lsfg::REASON_DISABLED),
        (LsfgSetup::Status(s), _) => s.clone(),
        (LsfgSetup::Detect { .. }, None) => LsfgStatus::unavailable(media::lsfg::REASON_ENGINE),
        (LsfgSetup::Detect { dll }, Some(_)) => {
            let dll = dll.as_ref().map(|p| p.to_string_lossy().into_owned());
            tokio::task::spawn_blocking(move || media::lsfg::detect_system(dll.as_deref()))
                .await
                .unwrap_or_else(|_| LsfgStatus::unavailable(media::lsfg::REASON_PROBE))
        }
    };
    let mut gpu = cfg.gpu.clone().unwrap_or_else(|| guess_gpu_with(&detected, render_node.as_ref()));
    if cfg.gpu.is_none() {
        gpu.vulkan = detected.libplacebo;
        if gpu.name.is_empty()
            && let Some(device) = lsfg.device()
        {
            gpu.name = device.to_owned();
        }
    }
    let anime4k_large_ok = shaders_ok
        && cfg.anime4k_dir.as_deref().is_some_and(|d| {
            media::anime4k::chain_present(d, media::anime4k::Anime4kMode::A, media::anime4k::Anime4kVariant::L)
        });
    let env = Arc::new(Environment { render_node, lsfg, anime4k_large_ok, ..Environment::new(detected, gpu, shaders_ok) });

    let engine: Arc<dyn Engine> = match (&cfg.engine, &ffmpeg) {
        (Some(e), _) => e.clone(),
        (None, Some(p)) => Arc::new(FfmpegEngine::new(p.clone())?),
        (None, None) => Arc::new(NoEngine),
    };
    let http = cfg.fetch_http.clone().unwrap_or_else(|| SafeHttp::new(Policy::link_source(cfg.allow_http_sources)));
    let fetcher = Arc::new(fetcher::Fetcher::start(http).await?);
    let (events, _) = broadcast::channel(64);
    let work_root = cfg.data_dir.join("sessions");
    let _ = std::fs::remove_dir_all(&work_root); // leftovers of a previous run: a fixed subdirectory of the link dir
    std::fs::create_dir_all(&work_root)?;
    let manager = sessions::Manager::new(
        cfg.tuning.clone(),
        engine,
        fetcher.clone(),
        (*env).clone(),
        if shaders_ok { cfg.anime4k_dir.clone() } else { None },
        work_root,
        cfg.allow_http_sources,
        host.clone(),
        events.clone(),
    );
    // BUG-025: sessions measure whether their profile keeps up and store it for `measured_sustain`
    let bench = Arc::new(Mutex::new(BenchStore::open(cfg.data_dir.join("bench.json"))));
    manager.set_bench(bench.clone());
    let (_, sink_task) = sink::start(manager.clone()).await?;

    let addrs: Vec<SocketAddr> = listeners.iter().filter_map(|l| l.local_addr().ok()).collect();
    let pair_host = bind_ips.iter().copied().find(|i| !i.is_loopback()).unwrap_or(Ipv4Addr::LOCALHOST);
    let pairing = Arc::new(PairingWindow::new(identity.clone(), devices.clone(), pair_host, port, cfg.server_name.clone()));
    let state = AppState {
        identity,
        devices: devices.clone(),
        pairing,
        manager: manager.clone(),
        env,
        bench: bench.clone(),
        host,
        instance_id: uuid::Uuid::new_v4().to_string(),
        server_name: cfg.server_name,
        client_fg_allowed: cfg.client_fg_allowed,
        events: events.clone(),
    };

    let shutdown = CancellationToken::new();
    let limit = Arc::new(Semaphore::new(server::MAX_CONNECTIONS));
    let mut tasks = Vec::new();
    for l in listeners {
        tasks.push(tokio::spawn(server::serve(l, tls.clone(), state.clone(), limit.clone(), shutdown.clone())));
    }
    // idle reaper + revoked-device session cleanup
    {
        let reaper = manager.clone();
        let tick = (cfg.tuning.idle_timeout / 4).clamp(std::time::Duration::from_millis(50), std::time::Duration::from_secs(5));
        let stop = shutdown.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! { () = stop.cancelled() => return, () = tokio::time::sleep(tick) => {} }
                reaper.reap_idle();
            }
        }));
        let manager = manager.clone();
        let mut revoked = devices.subscribe_revocations();
        let stop = shutdown.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    () = stop.cancelled() => return,
                    r = revoked.recv() => match r {
                        Ok(id) => manager.close_for_device(&id),
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(broadcast::error::RecvError::Closed) => return,
                    }
                }
            }
        }));
    }
    tracing::info!(addrs = ?addrs, "Tatakai Link listening");
    Ok(LinkHandle { state, addrs, shutdown, tasks, sink_task, fetcher })
}

impl LinkHandle {
    /// Open (or replace) the pairing window: new code, 5 minutes, single use, 5 attempts.
    pub fn open_pairing_window(&self) -> PairingPayload {
        self.state.pairing.open(DEFAULT_TTL, DEFAULT_MAX_ATTEMPTS, OffsetDateTime::now_utc())
    }

    pub fn close_pairing_window(&self) {
        self.state.pairing.close();
    }

    pub fn pairing_status(&self) -> WindowStatus {
        self.state.pairing.status(OffsetDateTime::now_utc())
    }

    pub fn devices(&self) -> Vec<Device> {
        self.state.devices.list()
    }

    /// Revoke a device: effective on its next request, live connections are closed and its session is torn down.
    pub fn revoke(&self, id: &str) {
        if self.state.devices.revoke(id, OffsetDateTime::now_utc().unix_timestamp()) {
            self.state.manager.close_for_device(id);
            let _ = self.state.events.send(LinkEvent::DeviceRevoked { device_id: id.to_owned() });
        }
    }

    /// 2026-11 history per device: set how this PC treats `id`'s history (persisted in `devices.json`).
    pub fn set_history_mode(&self, id: &str, mode: devices::HistoryMode) -> bool {
        self.state.devices.set_history_mode(id, mode)
    }

    /// "Use this device as the source": ask `id` to push its whole history and let those rows win for
    /// [`devices::RESYNC_WINDOW_SECS`]; its `resync-done` then asks the other devices to replace theirs. `false` when the
    /// device is unknown, revoked or not in `full` mode.
    pub fn start_resync_from(&self, id: &str) -> bool {
        self.state.devices.start_resync(id, OffsetDateTime::now_utc().unix_timestamp())
    }

    /// Whether `id`'s authoritative window is open (a resync from it is running).
    pub fn resync_running(&self, id: &str) -> bool {
        self.state.devices.resync_open(id, OffsetDateTime::now_utc().unix_timestamp())
    }

    pub fn rename_device(&self, id: &str, name: &str) -> bool {
        self.state.devices.rename(id, &pairing::sanitize_device_name(name))
    }

    pub fn events(&self) -> broadcast::Receiver<LinkEvent> {
        self.state.events.subscribe()
    }

    /// Addresses the server listens on.
    pub fn local_addrs(&self) -> Vec<SocketAddr> {
        self.addrs.clone()
    }

    pub fn port(&self) -> u16 {
        self.addrs.first().map_or(0, SocketAddr::port)
    }

    pub fn ca_fingerprint(&self) -> String {
        self.state.identity.ca_fingerprint()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.state.capabilities()
    }

    /// Store the verdict of a 10 s sustain benchmark for a profile (`measured_sustain` in `/v1/capabilities`).
    pub fn record_sustain(&self, profile: &str, sustained: bool) {
        self.state.bench.lock().unwrap_or_else(std::sync::PoisonError::into_inner).record(profile, sustained);
    }

    pub fn active_sessions(&self) -> usize {
        self.state.manager.active_count()
    }

    /// Device ids connected now ("Conectado ahora" in the app). BUG-039: a phone with a live transcoding session, or
    /// one whose last authenticated request (the `GET /v1/events` long-poll it keeps open while the app is in the
    /// foreground, every [`control::EVENTS_MAX_WAIT_SECS`] s at most) is at most [`ONLINE_WINDOW_SECS`] old.
    pub fn online_device_ids(&self) -> Vec<String> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        online_ids(self.state.manager.device_ids(), &self.state.devices.list(), now)
    }

    /// Stop listening and close every session without consuming the handle (for a handle shared through `Arc`).
    /// The listener tasks end on their own shortly after; [`LinkHandle::shutdown`] also waits for them.
    pub fn stop(&self) {
        self.shutdown.cancel();
        self.state.manager.close_all();
        self.sink_task.abort();
    }

    /// Stop listening, close every session (FFmpeg dies with them) and join the tasks.
    pub async fn shutdown(self) {
        self.stop();
        for t in self.tasks {
            let _ = t.await;
        }
        drop(self.fetcher);
    }
}

#[cfg(test)]
mod online_tests {
    use super::*;

    fn dev(id: &str, seen: Option<i64>) -> Device {
        Device { id: id.into(), name: id.into(), fingerprint: String::new(), created_at: 0, not_after: 10_000, revoked_at: None, platform: String::new(), last_seen_at: seen, history_mode: devices::HistoryMode::Full }
    }

    #[test]
    fn a_phone_long_polling_events_is_online_without_a_session() {
        let devices = vec![dev("tab", Some(1_000 - 19 * 3600)), dev("moto", Some(990)), dev("never", None)];
        assert_eq!(online_ids(vec![], &devices, 1_000), vec!["moto".to_owned()]);
        assert_eq!(online_ids(vec!["tab".into()], &devices, 1_000), vec!["tab".to_owned(), "moto".to_owned()], "a session counts too, once");
        assert!(online_ids(vec![], &devices, 990 + ONLINE_WINDOW_SECS + 1).is_empty(), "stale after the window");
        let mut revoked = dev("old", Some(999));
        revoked.revoked_at = Some(500);
        assert!(online_ids(vec![], &[revoked], 1_000).is_empty());
    }
}
