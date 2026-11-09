//! Link server glue over tk-link (`start(LinkConfig, Arc<dyn LinkHost>)` -> `LinkHandle`).
//!
//! The server starts the first time the Link screen asks for status (so the OS firewall prompt never
//! appears at first launch - review E6) and runs until the process exits or the user restarts it on
//! another port (`restart`). A failed start (port taken: os error 10048 / 98) is not an `Err` of
//! `status` / `restart`: it is reported in `LinkStatusVm::error` so the screen can offer the fixes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tk_core::LocalStore;
use tk_ext::ExtensionHost;
use tk_link::media::capabilities::Sustain;
use tk_link::{Device, LinkConfig, LinkError, LinkHandle, LinkHost, Vendor};
use tk_ui::backend::{DeviceVm, LinkCapsVm, LinkStatusVm, RemoteCommandVm, SendToDeviceVm, UiResult};
use tk_ui::model::{ErrorKind, Pairing, UiError};

/// What the Link server may ask of the app. M0: no library folder; FFmpeg only if `fetch-deps.ps1` ran.
/// Extension permissions come from the installed extension, so `url` sources can only reach its declared domains.
/// Wave A (worker f): the watch history for `GET` / `POST /v1/history` (`super::history_sync`).
struct AppLinkHost {
    ext: Arc<ExtensionHost>,
    store: Arc<LocalStore>,
    /// Wave B (worker k): commands from `POST /v1/control` for `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
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
        crate::paths::ffmpeg_path(None)
    }
    fn control(&self, cmd: tk_link::control::ControlCommand) -> bool {
        self.remote.send(remote_vm(cmd)).is_ok()
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

/// `Backend::send_to_device` (wave B, worker **k**; checklist `link.send-to-device`). Contract stub: k creates a Link
/// session for the device (`tk-link::sessions`, the same path the phone uses when it asks for a stream) and pushes a
/// `handoff` event the phone consumes (`/v1/events`, k adds it to `tk-link/src/server.rs`), then the phone opens the
/// player at `position_secs`. Until then `NotWired`, which Details shows inline (KNOWN-ISSUES, wave A).
pub async fn send_to_device(b: &LiveBackend, req: &SendToDeviceVm) -> UiResult<()> {
    let _ = (b, req);
    Err(UiError::not_wired("send_to_device"))
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
    state: tokio::sync::Mutex<LinkState>,
    /// Wave B (k): handed to every `AppLinkHost` so `/v1/control` reaches `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
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
    // tk-link tracks no platform / last-seen; the pairing date is not "last seen", so leave it empty.
    let online = online.iter().any(|id| *id == d.id);
    DeviceVm { id: d.id, name: d.name, platform: String::new(), last_seen_unix: None, online }
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
        remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    ) -> Self {
        let port = load_port(&dir);
        Self { dir, ext, store, anime4k_dir, anime4k_lock, state: tokio::sync::Mutex::new(LinkState { handle: None, port }), remote }
    }

    /// Start on `st.port`.
    async fn start_locked(&self, st: &mut LinkState) -> Result<Arc<LinkHandle>, UiError> {
        let mut cfg = LinkConfig::new(self.dir.clone(), server_name());
        cfg.port = st.port;
        cfg.anime4k_dir = Some(self.anime4k_dir.clone());
        cfg.anime4k_lock = self.anime4k_lock.clone();
        let host = Arc::new(AppLinkHost { ext: self.ext.clone(), store: self.store.clone(), remote: self.remote.clone() });
        match tk_link::start(cfg, host).await {
            Ok(h) => {
                let h = Arc::new(h);
                st.handle = Some(h.clone());
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
