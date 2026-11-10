//! Link server glue over tk-link (`start(LinkConfig, Arc<dyn LinkHost>)` -> `LinkHandle`).
//!
//! The server starts the first time the Link screen asks for status (so the OS firewall prompt never
//! appears at first launch - review E6) and runs until the process exits or the user restarts it on
//! another port (`restart`). A failed start (port taken: os error 10048 / 98) is not an `Err` of
//! `status` / `restart`: it is reported in `LinkStatusVm::error` so the screen can offer the fixes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tk_core::LocalStore;
use tk_ext::ExtensionHost;
use tk_link::control::{PhoneEvent, PlayerState};
use tk_link::discovery::Announcer;
use tk_link::media::capabilities::Sustain;
use tk_link::sessions::{CreateRequest, SourceRequest};
use tk_link::{Device, LinkConfig, LinkError, LinkHandle, LinkHost, Vendor};
use tk_ui::backend::{Backend, DeviceVm, LinkCapsVm, LinkStatusVm, PresenceVm, RemoteCommandVm, SendToDeviceVm, UiResult};
use tk_ui::model::{ErrorKind, Pairing, UiError};

use super::LiveBackend;

/// A player state older than this is treated as "no player" (the player reports every 15 s and on every pause
/// change, `screens/player/presence.rs`).
const PLAYER_STALE: Duration = Duration::from_secs(45);

/// What the PC's player shows, as last reported (`note_presence` / `note_progress`), for `GET /v1/player`.
#[derive(Default)]
struct PlayerTrack {
    state: Option<(Instant, PlayerState)>,
}

type SharedPlayer = Arc<Mutex<PlayerTrack>>;

/// `GET /v1/player` answer from the tracked state (`None` when stale or closed). Pure apart from the clock.
fn current_player(track: &SharedPlayer) -> Option<PlayerState> {
    let g = track.lock().ok()?;
    g.state.as_ref().filter(|(at, _)| at.elapsed() < PLAYER_STALE).map(|(_, s)| s.clone())
}

/// Merge a presence report into the tracked state (`Watching` keeps the media id of the last progress row of
/// the same title / episode; anything else closes the player). Pure.
fn merge_presence(prev: Option<&PlayerState>, state: &PresenceVm) -> Option<PlayerState> {
    match state {
        PresenceVm::Watching { title, episode, position_secs, duration_secs, paused, .. } => {
            let media_id = prev.filter(|p| p.episode == Some(*episode)).and_then(|p| p.media_id.clone());
            Some(PlayerState {
                playing: !*paused,
                title: Some(title.clone()).filter(|t| !t.is_empty()),
                episode: Some(*episode),
                position_secs: *position_secs,
                duration_secs: *duration_secs,
                media_id,
            })
        }
        _ => None,
    }
}

/// Whether a transport command changes nothing given the player state (`play` while playing, `pause` while
/// paused): it is accepted without reaching the player, whose only primitive is "toggle". Pure.
fn redundant(cmd: &tk_link::control::ControlCommand, st: Option<&PlayerState>) -> bool {
    use tk_link::control::ControlCommand as C;
    match (cmd, st) {
        (C::Play, Some(s)) => s.playing,
        (C::Pause, Some(s)) => !s.playing,
        _ => false,
    }
}

/// What the Link server may ask of the app. M0: no library folder; FFmpeg only if `fetch-deps.ps1` ran.
/// Extension permissions come from the installed extension, so `url` sources can only reach its declared domains.
/// Wave A (worker f): the watch history for `GET` / `POST /v1/history` (`super::history_sync`).
struct AppLinkHost {
    ext: Arc<ExtensionHost>,
    store: Arc<LocalStore>,
    /// Wave B (worker k): commands from `POST /v1/control` for `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    /// Wave B (worker k): the PC player as last reported, for `GET /v1/player`.
    player: SharedPlayer,
}

impl LinkHost for AppLinkHost {
    fn extension_permissions(&self, id: &str) -> Option<Vec<String>> {
        self.ext.get(id).map(|e| e.permissions)
    }
    fn history_since(&self, since_unix: i64, limit: usize) -> Vec<tk_core::HistoryEntry> {
        super::history_sync::since(&self.store, since_unix, limit)
    }
    fn history_merge(&self, rows: Vec<tk_core::HistoryEntry>) -> usize {
        super::history_sync::merge(&self.store, rows)
    }
    fn history_clear(&self) -> bool {
        super::history_sync::clear(&self.store)
    }
    fn library(&self) -> Vec<tk_link::LibraryItem> {
        Vec::new()
    }
    fn library_path(&self, _id: &str) -> Option<PathBuf> {
        None
    }
    fn ffmpeg_path(&self) -> Option<PathBuf> {
        // the bundled build first (`<exe>/ffmpeg/bin`); on Linux the distribution's FFmpeg (VAAPI, libplacebo)
        crate::paths::ffmpeg_path(None).or_else(|| if cfg!(windows) { None } else { find_in_path("ffmpeg") })
    }
    fn control(&self, cmd: tk_link::control::ControlCommand) -> bool {
        let st = current_player(&self.player);
        if redundant(&cmd, st.as_ref()) {
            return true;
        }
        self.remote.send(remote_vm(cmd)).is_ok()
    }
    fn player_state(&self) -> Option<PlayerState> {
        current_player(&self.player)
    }
}

/// The player reported its presence (`Backend::presence`; integrator line in `LiveBackend::presence`): track it for
/// `GET /v1/player` and tell the remote-seek mapping where the player is (`screens/player/remote.rs`).
pub fn note_presence(b: &LiveBackend, state: &PresenceVm) {
    if let PresenceVm::Watching { position_secs, .. } = state {
        tk_ui::screens::player::remote::note_position(*position_secs);
    }
    if let Ok(mut g) = b.link.player.lock() {
        let prev = g.state.as_ref().map(|(_, s)| s.clone());
        g.state = merge_presence(prev.as_ref(), state).map(|s| (Instant::now(), s));
    }
}

/// The player saved progress (`Backend::record_progress`; integrator line in `LiveBackend::record_progress`): the
/// media id and position of what is playing.
pub fn note_progress(b: &LiveBackend, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) {
    tk_ui::screens::player::remote::note_position(progress_secs);
    if let Ok(mut g) = b.link.player.lock() {
        let mut st = g.state.as_ref().map(|(_, s)| s.clone()).unwrap_or(PlayerState {
            playing: true,
            title: None,
            episode: Some(episode),
            position_secs: progress_secs,
            duration_secs,
            media_id: None,
        });
        st.media_id = Some(id.to_owned());
        st.episode = Some(episode);
        st.position_secs = progress_secs;
        if duration_secs.is_some() {
            st.duration_secs = duration_secs;
        }
        g.state = Some((Instant::now(), st));
    }
}

/// Wire command (`tk-link::control`) -> UI command. Pure; tested below.
fn remote_vm(cmd: tk_link::control::ControlCommand) -> RemoteCommandVm {
    use tk_link::control::ControlCommand as C;
    match cmd {
        C::Play => RemoteCommandVm::Play,
        C::Pause => RemoteCommandVm::Pause,
        C::Toggle => RemoteCommandVm::TogglePause,
        C::Next => RemoteCommandVm::NextEpisode,
        C::Prev => RemoteCommandVm::PrevEpisode,
        C::Seek { seconds } => RemoteCommandVm::Seek(seconds),
        C::Open { media_id, episode, position_secs } => RemoteCommandVm::Open { media_id, episode, position_secs },
    }
}

/// The Link session source for a resolved stream: only public http(s) URLs travel (the phone's session fetches
/// them through the SSRF-safe loopback fetcher of the PC). Pure.
fn session_source(url: &str, headers: &[(String, String)]) -> Option<SourceRequest> {
    let u = reqwest::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?;
    if host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return None;
    }
    Some(SourceRequest::Url {
        url: url.to_owned(),
        headers: headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        extension_id: None,
        container: None,
    })
}

fn link_api_err(e: tk_link::api::ApiError) -> UiError {
    let kind = match e.0 {
        tk_link::api::ErrorCode::DeviceNotAuthorized => ErrorKind::Auth,
        tk_link::api::ErrorCode::SourceUnavailable | tk_link::api::ErrorCode::UrlSourceDenied => ErrorKind::NoSource,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, format!("{}: {}", e.0.as_str(), e.0.message()))
}

/// `Backend::send_to_device` (wave B, worker **k**; checklist `link.send-to-device`): resolve the episode on the PC
/// (`Backend::playback`, the same sources the PC player would use), start a Link session owned by the phone at
/// `position_secs` (`tk-link::sessions`, the path the phone itself uses with `POST /v1/sessions`) and queue the
/// `handoff` event its `GET /v1/events` long-poll delivers ("La PC te envió <título>" -> "Reproducir aquí").
pub async fn send_to_device(b: &LiveBackend, req: &SendToDeviceVm) -> UiResult<()> {
    let h = b.link.ensure().await?;
    let device = h
        .devices()
        .into_iter()
        .find(|d| d.id == req.device_id && d.is_active(now_unix()))
        .ok_or_else(|| UiError::new(ErrorKind::Auth, format!("device {} is not paired", req.device_id)))?;
    let playback = <LiveBackend as Backend>::playback(b, &req.media_id, req.episode).await?;
    let mut last: Option<UiError> = None;
    for stream in &playback.streams {
        let Some(source) = session_source(&stream.url, &stream.headers) else { continue };
        let create = CreateRequest { source, profile: None, audio_track: 0, start_secs: f64::from(req.position_secs) };
        match h.create_session_for(&device.id, create).await {
            Ok(created) => {
                let event = PhoneEvent::Handoff {
                    session_id: created.session_id,
                    media_id: Some(req.media_id.clone()),
                    episode: Some(req.episode),
                    position_secs: req.position_secs,
                    title: Some(playback.title.clone()).filter(|t| !t.is_empty()),
                };
                if !h.push_event(&device.id, event) {
                    return Err(UiError::new(ErrorKind::Auth, format!("device {} is not paired", device.id)));
                }
                tracing::info!(device = %device.id, media = %req.media_id, episode = req.episode, "sent to device");
                return Ok(());
            }
            Err(e) => {
                tracing::debug!(error = ?e.0, "send to device: session refused, trying the next stream");
                last = Some(link_api_err(e));
            }
        }
    }
    Err(last.unwrap_or_else(|| UiError::new(ErrorKind::NoSource, "no stream of this episode can be sent to the phone")))
}

/// `Settings::server.link_announce` changed (or start-up): announce Link over mDNS or stop (`link.discovery`).
pub async fn set_announce(b: &LiveBackend, on: bool) {
    b.link.announce.store(on, Ordering::Relaxed);
    let st = b.link.state.lock().await;
    let handle = st.handle.clone();
    drop(st);
    match (on, handle) {
        (true, Some(h)) => b.link.start_announcer(&h),
        _ => b.link.stop_announcer(),
    }
}

/// `Backend::wait_remote_command` (wave B, worker **k**; `link.remote-control`, `link.handoff`): the next command a
/// paired phone posted to `/v1/control`. Implemented: it awaits the channel `AppLinkHost::control` feeds. The route
/// handler itself is k's (`tk-link/src/server.rs`); until k adds it, nothing arrives and the shell simply waits.
pub async fn wait_remote_command(b: &LiveBackend) -> UiResult<RemoteCommandVm> {
    let mut rx = b.remote.lock().await;
    rx.recv().await.ok_or_else(|| UiError::new(ErrorKind::Other, "remote command channel closed"))
}

/// The running server (if any) and the port it uses or will try next.
struct LinkState {
    handle: Option<Arc<LinkHandle>>,
    port: u16,
}

pub struct LinkService {
    dir: PathBuf,
    ext: Arc<ExtensionHost>,
    store: Arc<LocalStore>,
    anime4k_dir: PathBuf,
    anime4k_lock: Option<PathBuf>,
    /// `Lossless.dll` for the LSFG profiles ([`link_lsfg_dll`]); `None` = Steam lookup by tk-link.
    lsfg_dll: Option<PathBuf>,
    state: tokio::sync::Mutex<LinkState>,
    /// Wave B (k): handed to every `AppLinkHost` so `/v1/control` reaches `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    /// Wave B (k): the player state shared with every `AppLinkHost` (`GET /v1/player`).
    player: SharedPlayer,
    /// Wave B (k, `link.discovery`): announce over mDNS while the server runs (`Settings::server.link_announce`).
    announce: AtomicBool,
    announcer: Mutex<Option<Announcer>>,
}

/// `name` in one of the absolute `PATH` entries.
fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).filter(|d| d.is_absolute()).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The `Lossless.dll` for Link's LSFG helper from the player's "Lossless.dll" setting. temple-lsfg reads the game's
/// `Lossless.dll` (not the `lsfg-vk.dll` pack lsfg-vk 2.x wants): a setting that names another file is used only when
/// a `Lossless.dll` sits next to it; otherwise Link searches Steam itself.
pub fn link_lsfg_dll(setting: Option<&str>) -> Option<PathBuf> {
    let p = PathBuf::from(setting.map(str::trim).filter(|s| !s.is_empty())?);
    if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Lossless.dll")) {
        return Some(p);
    }
    let sibling = p.with_file_name("Lossless.dll");
    sibling.is_file().then_some(sibling)
}

/// The chosen port lives next to the Link identity (`<link>/port`); missing or invalid = the default.
const PORT_FILE: &str = "port";

fn load_port(dir: &Path) -> u16 {
    std::fs::read_to_string(dir.join(PORT_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .unwrap_or(tk_link::DEFAULT_PORT)
}

fn save_port(dir: &Path, port: u16) {
    let write = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(PORT_FILE), format!("{port}\n")));
    if let Err(e) = write {
        tracing::warn!(error = %e, "could not persist the Link port");
    }
}

fn server_name() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()).map_or_else(|| "Tatakai".to_owned(), |n| format!("Tatakai ({n})"))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn is_addr_in_use(e: &std::io::Error) -> bool {
    // WSAEADDRINUSE (10048) / EADDRINUSE (98 on Linux); std maps both to AddrInUse, the codes are a fallback.
    e.kind() == std::io::ErrorKind::AddrInUse || matches!(e.raw_os_error(), Some(10048 | 98))
}

/// Start failure -> `UiError`. A taken port is `AddrInUse` with the OS message as detail.
fn start_err(e: &LinkError, port: u16) -> UiError {
    match e {
        LinkError::Io(io) if is_addr_in_use(io) => UiError::new(ErrorKind::AddrInUse, format!("io: {io} · bind :{port}")),
        _ => err(e),
    }
}

fn device_vm(d: Device, online: &[String]) -> DeviceVm {
    // Wave B (k, `link.last-seen`): tk-link refreshes both on every authenticated request of the phone.
    let online = online.iter().any(|id| *id == d.id);
    DeviceVm { id: d.id, name: d.name, platform: d.platform, last_seen_unix: d.last_seen_at, online }
}

/// `7A3F 91C2 0E5D B8A4`: the first 16 hex digits of the CA fingerprint, upper case, in groups of 4.
fn display_fingerprint(hex: &str) -> String {
    let digits: Vec<char> = hex.chars().filter(char::is_ascii_hexdigit).take(16).map(|c| c.to_ascii_uppercase()).collect();
    digits.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

fn vendor_label(v: Vendor) -> Option<&'static str> {
    match v {
        Vendor::Amd => Some("AMD"),
        Vendor::Nvidia => Some("NVIDIA"),
        Vendor::Intel => Some("Intel"),
        Vendor::Other | Vendor::Unknown => None,
    }
}

/// Stop a shared handle. The handle is only cloned for the duration of a call, so it is normally unique by now;
/// if not, stop it in place and give the listener tasks a moment to release the port.
async fn stop_handle(mut h: Arc<LinkHandle>) {
    for _ in 0..20 {
        match Arc::try_unwrap(h) {
            Ok(owned) => {
                owned.shutdown().await;
                return;
            }
            Err(shared) => {
                h = shared;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    h.stop();
    tokio::time::sleep(Duration::from_millis(150)).await;
}

impl LinkService {
    pub fn new(
        dir: PathBuf,
        ext: Arc<ExtensionHost>,
        store: Arc<LocalStore>,
        anime4k_dir: PathBuf,
        anime4k_lock: Option<PathBuf>,
        lsfg_dll: Option<PathBuf>,
        remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    ) -> Self {
        let port = load_port(&dir);
        Self {
            dir,
            ext,
            store,
            anime4k_dir,
            anime4k_lock,
            lsfg_dll,
            state: tokio::sync::Mutex::new(LinkState { handle: None, port }),
            remote,
            player: SharedPlayer::default(),
            announce: AtomicBool::new(true),
            announcer: Mutex::new(None),
        }
    }

    /// (Re)register the mDNS service for `h` (best effort: a failure only means phones must scan the QR).
    fn start_announcer(&self, h: &LinkHandle) {
        let ips: Vec<std::net::IpAddr> = h.local_addrs().iter().map(std::net::SocketAddr::ip).collect();
        let next = match Announcer::start_on(&h.server_name(), &h.instance_id(), h.port(), &h.ca_fingerprint(), &ips) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!(error = %e, "mDNS announcement failed");
                None
            }
        };
        if let Ok(mut g) = self.announcer.lock() {
            *g = next; // the previous one (if any) unregisters on drop
        }
    }

    fn stop_announcer(&self) {
        if let Ok(mut g) = self.announcer.lock() {
            g.take();
        }
    }

    /// Start on `st.port`.
    async fn start_locked(&self, st: &mut LinkState) -> Result<Arc<LinkHandle>, UiError> {
        let mut cfg = LinkConfig::new(self.dir.clone(), server_name());
        cfg.port = st.port;
        cfg.anime4k_dir = Some(self.anime4k_dir.clone());
        cfg.anime4k_lock = self.anime4k_lock.clone();
        // LSFG (experimental): temple-lsfg via TEMPLE_LSFG or next to the executable; probed once per server start
        cfg.lsfg = tk_link::LsfgSetup::Detect { dll: self.lsfg_dll.clone() };
        let host = Arc::new(AppLinkHost {
            ext: self.ext.clone(),
            store: self.store.clone(),
            remote: self.remote.clone(),
            player: self.player.clone(),
        });
        match tk_link::start(cfg, host).await {
            Ok(h) => {
                let h = Arc::new(h);
                st.handle = Some(h.clone());
                if self.announce.load(Ordering::Relaxed) {
                    self.start_announcer(&h);
                }
                Ok(h)
            }
            Err(e) => {
                tracing::warn!(port = st.port, error = %e, "Tatakai Link did not start");
                Err(start_err(&e, st.port))
            }
        }
    }

    async fn ensure(&self) -> Result<Arc<LinkHandle>, UiError> {
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.as_ref() {
            return Ok(h.clone());
        }
        self.start_locked(&mut st).await
    }

    fn running_vm(h: &LinkHandle) -> LinkStatusVm {
        let online = h.online_device_ids();
        LinkStatusVm {
            running: true,
            port: h.port(),
            devices: h.devices().into_iter().filter(|d| d.is_active(now_unix())).map(|d| device_vm(d, &online)).collect(),
            error: None,
        }
    }

    /// Server down: the paired devices still come from `devices.json`, none of them online.
    fn stopped_vm(&self, port: u16, error: UiError) -> LinkStatusVm {
        let devices = tk_link::devices::DeviceRegistry::open(self.dir.join("devices.json"))
            .map(|r| r.list())
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.is_active(now_unix()))
            .map(|d| device_vm(d, &[]))
            .collect();
        LinkStatusVm { running: false, port, devices, error: Some(error) }
    }

    /// App start-up: with at least one paired phone the server starts right away, so the phone reaches this PC without
    /// the user opening the Link screen first. With none it stays lazy (no listening port until pairing).
    pub async fn start_if_paired(&self) {
        let paired = tk_link::devices::DeviceRegistry::open(self.dir.join("devices.json"))
            .map(|r| r.list().into_iter().any(|d| d.is_active(now_unix())))
            .unwrap_or(false);
        if paired && let Err(e) = self.ensure().await {
            tracing::warn!(detail = %e.detail, "Tatakai Link could not start");
        }
    }

    /// Starts the server if it is not running (each call retries a failed start).
    pub async fn status(&self) -> Result<LinkStatusVm, UiError> {
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.clone() {
            return Ok(Self::running_vm(&h));
        }
        match self.start_locked(&mut st).await {
            Ok(h) => Ok(Self::running_vm(&h)),
            Err(e) => Ok(self.stopped_vm(st.port, e)),
        }
    }

    /// Stop the server (if running) and start it on `port`. The port is persisted only when the start works.
    pub async fn restart(&self, port: u16) -> Result<LinkStatusVm, UiError> {
        if port == 0 {
            return Err(err("port 0 is not a valid Link port"));
        }
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.take() {
            self.stop_announcer();
            stop_handle(h).await;
        }
        st.port = port;
        match self.start_locked(&mut st).await {
            Ok(h) => {
                save_port(&self.dir, h.port());
                Ok(Self::running_vm(&h))
            }
            Err(e) => Ok(self.stopped_vm(port, e)),
        }
    }

    /// The port the server uses (or will try), for the firewall rule.
    pub async fn port(&self) -> u16 {
        let st = self.state.lock().await;
        st.handle.as_ref().map_or(st.port, |h| h.port())
    }

    pub async fn open_pairing(&self) -> Result<Pairing, UiError> {
        let p = self.ensure().await?.open_pairing_window();
        Ok(Pairing { uri: p.uri(), code: p.code, expires_unix: p.expires_at.unix_timestamp() })
    }

    pub async fn revoke(&self, id: &str) -> Result<(), UiError> {
        self.ensure().await?.revoke(id);
        Ok(())
    }

    /// Persisted in tk-link `devices.json` (the name is sanitized there).
    pub async fn rename(&self, id: &str, name: &str) -> Result<(), UiError> {
        if self.ensure().await?.rename_device(id, name) {
            Ok(())
        } else {
            Err(err(format!("device {id} could not be renamed (unknown, revoked or empty name)")))
        }
    }

    /// What `GET /v1/capabilities` answers, read from the running server (no network round trip).
    pub async fn capabilities(&self) -> Result<LinkCapsVm, UiError> {
        let h = self.ensure().await?;
        let caps = h.capabilities();
        let gpu = Some(caps.gpu.name.trim().to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| vendor_label(caps.gpu.vendor).map(str::to_owned));
        Ok(LinkCapsVm {
            gpu,
            // tk-link only knows whether Vulkan works through libplacebo, not the API version.
            vulkan: caps.gpu.vulkan.then(|| "libplacebo".to_owned()),
            encoder: caps.encoders.first().map(|e| (*e).to_owned()),
            upscale: caps.profiles.iter().any(|p| p.upscale == "anime4k").then(|| "Anime4K (libplacebo)".to_owned()),
            profiles: caps
                .profiles
                .iter()
                .map(|p| {
                    let measured = match p.measured_sustain {
                        Sustain::Yes => Some(true),
                        Sustain::No => Some(false),
                        Sustain::Unknown => None,
                    };
                    (p.id.to_owned(), measured)
                })
                .collect(),
            fingerprint: display_fingerprint(&h.ca_fingerprint()),
            lan_ip: h.local_addrs().iter().map(std::net::SocketAddr::ip).find(|ip| !ip.is_loopback()).map(|ip| ip.to_string()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lsfg_dll_setting_must_be_the_games_lossless_dll() {
        assert_eq!(link_lsfg_dll(None), None);
        assert_eq!(link_lsfg_dll(Some("  ")), None);
        assert_eq!(link_lsfg_dll(Some("/s/Lossless Scaling/Lossless.dll")), Some(PathBuf::from("/s/Lossless Scaling/Lossless.dll")));
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("lsfg-vk.dll");
        assert_eq!(link_lsfg_dll(Some(pack.to_str().unwrap())), None, "no Lossless.dll next to the lsfg-vk pack");
        std::fs::write(dir.path().join("Lossless.dll"), b"x").unwrap();
        assert_eq!(link_lsfg_dll(Some(pack.to_str().unwrap())), Some(dir.path().join("Lossless.dll")));
    }

    #[test]
    fn control_commands_map_to_ui_commands() {
        use tk_link::control::ControlCommand as C;
        assert_eq!(remote_vm(C::Toggle), RemoteCommandVm::TogglePause);
        assert_eq!(remote_vm(C::Seek { seconds: 90 }), RemoteCommandVm::Seek(90));
        assert_eq!(
            remote_vm(C::Open { media_id: "21".into(), episode: 3, position_secs: 120 }),
            RemoteCommandVm::Open { media_id: "21".into(), episode: 3, position_secs: 120 }
        );
    }

    #[test]
    fn player_tracking_and_redundant_commands() {
        use tk_link::control::ControlCommand as C;
        let watching = PresenceVm::Watching {
            title: "One Piece".into(),
            episode: 3,
            position_secs: 90,
            duration_secs: Some(1440),
            paused: true,
            poster: None,
        };
        let prev = PlayerState { playing: true, title: None, episode: Some(3), position_secs: 80, duration_secs: None, media_id: Some("21".into()) };
        let st = merge_presence(Some(&prev), &watching).unwrap();
        assert!(!st.playing);
        assert_eq!(st.media_id.as_deref(), Some("21"), "same episode keeps the media id");
        assert_eq!(st.position_secs, 90);
        let other_ep = PlayerState { episode: Some(2), ..prev.clone() };
        assert_eq!(merge_presence(Some(&other_ep), &watching).unwrap().media_id, None);
        assert_eq!(merge_presence(Some(&prev), &PresenceVm::Browsing), None, "leaving the player closes it");
        assert!(redundant(&C::Pause, Some(&st)));
        assert!(!redundant(&C::Play, Some(&st)));
        assert!(!redundant(&C::Toggle, Some(&st)));
        let track = SharedPlayer::default();
        assert_eq!(current_player(&track), None);
        track.lock().unwrap().state = Some((Instant::now(), st.clone()));
        assert_eq!(current_player(&track), Some(st));
    }

    #[test]
    fn only_public_streams_become_session_sources() {
        let h = vec![("Referer".to_owned(), "https://site/".to_owned())];
        match session_source("https://cdn.example/a.m3u8", &h) {
            Some(SourceRequest::Url { url, headers, .. }) => {
                assert_eq!(url, "https://cdn.example/a.m3u8");
                assert_eq!(headers.get("Referer").map(String::as_str), Some("https://site/"));
            }
            _ => panic!("expected a url source"),
        }
        assert!(session_source("http://127.0.0.1:8099/api/server/v1/media/x/a.m3u8", &[]).is_none());
        assert!(session_source("file:///home/a.mkv", &[]).is_none());
        assert!(session_source("not a url", &[]).is_none());
    }

    #[test]
    fn fingerprint_display_form() {
        assert_eq!(display_fingerprint("7a3f91c20e5db8a4ffff"), "7A3F 91C2 0E5D B8A4");
        assert_eq!(display_fingerprint("ab"), "AB");
    }

    #[test]
    fn port_file_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_port(tmp.path()), tk_link::DEFAULT_PORT);
        save_port(tmp.path(), 47331);
        assert_eq!(load_port(tmp.path()), 47331);
        std::fs::write(tmp.path().join(PORT_FILE), "nope").unwrap();
        assert_eq!(load_port(tmp.path()), tk_link::DEFAULT_PORT);
    }

    #[test]
    fn addr_in_use_maps_to_its_kind() {
        let e = LinkError::Io(std::io::Error::from(std::io::ErrorKind::AddrInUse));
        assert_eq!(start_err(&e, 47321).kind, ErrorKind::AddrInUse);
        assert_eq!(start_err(&LinkError::NoAddress, 47321).kind, ErrorKind::Other);
    }
}
