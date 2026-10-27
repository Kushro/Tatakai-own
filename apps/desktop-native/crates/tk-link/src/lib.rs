//! tk-link: Tatakai Link v1 server (PC side). See `docs/redesign/03-pairing-upscale.md` and
//! `contracts/link/openapi.v1.json`.
//!
//! * identity: own CA + server certificate (rcgen/ring), keys protected with DPAPI
//! * pairing: 5-minute single-use window, <= 5 attempts, Crockford code, `tatakai-link:v1?...` payload
//! * transport: rustls (ring) + optional client certs, axum routes, per-request device authz, hot revocation
//! * `url` source: SSRF-safe loopback fetcher + HLS rewriting; FFmpeg only ever reads the loopback
//! * sessions: on-demand fMP4 HLS, 512 MiB RAM window, heartbeat/idle, FFmpeg in a Windows Job Object
//!
//! Out of M0 by design: mDNS/TOFU, `/v1/settings`, events/handoff/control/sync/downloads, the phone source.

pub mod api;
pub mod devices;
pub mod fetcher;
pub mod firewall;
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
pub use pairing::{PairingPayload, WindowStatus};
pub use sessions::SessionTuning;

use crate::identity::Identity;
use crate::media::capabilities::{BenchStore, Environment, SystemRunner, detect, guess_gpu};
use crate::media::engine::{EngineJob, EngineProcess, FfmpegEngine};
use crate::pairing::{DEFAULT_MAX_ATTEMPTS, DEFAULT_TTL, PairingWindow};
use crate::server::AppState;
use tk_core::net::{Policy, SafeHttp};

pub const DEFAULT_PORT: u16 = 47321;

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
    /// Real GPU info from the app (DXGI). `None` = guessed from the detected encoders.
    pub gpu: Option<GpuInfo>,
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
    let mut gpu = cfg.gpu.clone().unwrap_or_else(|| guess_gpu(&detected));
    if cfg.gpu.is_none() {
        gpu.vulkan = detected.libplacebo;
    }
    let env = Arc::new(Environment { detected, gpu, shaders_ok });

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
    let (_, sink_task) = sink::start(manager.clone()).await?;

    // listeners
    let mut listeners = Vec::new();
    let mut port = cfg.port;
    for ip in &bind_ips {
        let l = TcpListener::bind((*ip, port)).await?;
        if port == 0 {
            port = l.local_addr()?.port();
        }
        listeners.push(l);
    }
    let addrs: Vec<SocketAddr> = listeners.iter().filter_map(|l| l.local_addr().ok()).collect();
    let pair_host = bind_ips.iter().copied().find(|i| !i.is_loopback()).unwrap_or(Ipv4Addr::LOCALHOST);
    let pairing = Arc::new(PairingWindow::new(identity.clone(), devices.clone(), pair_host, port, cfg.server_name.clone()));
    let state = AppState {
        identity,
        devices: devices.clone(),
        pairing,
        manager: manager.clone(),
        env,
        bench: Arc::new(Mutex::new(BenchStore::open(cfg.data_dir.join("bench.json")))),
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

    /// Stop listening, close every session (FFmpeg dies with them) and join the tasks.
    pub async fn shutdown(self) {
        self.shutdown.cancel();
        self.state.manager.close_all();
        self.sink_task.abort();
        for t in self.tasks {
            let _ = t.await;
        }
        drop(self.fetcher);
    }
}
