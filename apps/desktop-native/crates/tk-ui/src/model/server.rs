//! Native extension server, proxy / FlareSolverr and embed sniffer preferences and view-models (wave B, worker
//! **k**; docs/redesign/16-parity-wave-b-contract.md section 3.4). `ServerPrefs` is embedded in `Settings::server`
//! (`SettingsAction::SetServer`). Worker k owns this file; `model/mod.rs` stays frozen.

use serde::{Deserialize, Serialize};

use crate::backend::StreamVm;
use crate::model::UiError;

/// Classic default port of the integrated extension server (`server/`, `ExtensionHostPortSetting`).
pub const SERVER_DEFAULT_PORT: u16 = 8099;
pub const SERVER_MIN_PORT: u16 = 1024;
pub const SNIFF_MIN_SECS: u8 = 5;
pub const SNIFF_MAX_SECS: u8 = 60;

/// `Settings::server`: the PC-side extension server (`crates/tk-server`) and the embed sniffer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerPrefs {
    /// Start the server with the app (`server.native-mode`).
    pub enabled: bool,
    /// `1024..=65535`, default 8099 (classic).
    pub port: u16,
    /// Server name shown to phones; empty = this PC's host name.
    pub name: String,
    /// Phones must pair (one-time code) unless a viewer password is set (`server.pairing`).
    pub require_pairing: bool,
    /// Media proxy for the phones (`server.proxy`, `/api/server/v1/media/*`).
    pub proxy_enabled: bool,
    /// Extra CORS origins allowed by the server (the dashboard SPA is out of plan; kept for third-party clients).
    pub allowed_origins: Vec<String>,
    /// Per-source resolve timeout, seconds `5..=120`.
    pub timeout_secs: u8,
    /// Sources resolved in parallel, `1..=8`.
    pub concurrency: u8,
    /// FlareSolverr endpoint (`http://127.0.0.1:8191`) used for Cloudflare-protected sites; `None` = off.
    pub flaresolverr_url: Option<String>,
    /// Embed sources are opened in the hidden web view to capture their media URL (`sources.embed-sniffer`).
    pub sniffer_enabled: bool,
    /// Seconds the sniffer waits for a media request, `5..=60`.
    pub sniffer_timeout_secs: u8,
}

impl Default for ServerPrefs {
    fn default() -> Self {
        Self {
            enabled: false,
            port: SERVER_DEFAULT_PORT,
            name: String::new(),
            require_pairing: true,
            proxy_enabled: true,
            allowed_origins: Vec::new(),
            timeout_secs: 45,
            concurrency: 4,
            flaresolverr_url: None,
            sniffer_enabled: true,
            sniffer_timeout_secs: 20,
        }
    }
}

impl ServerPrefs {
    pub fn sanitized(mut self) -> Self {
        if self.port < SERVER_MIN_PORT {
            self.port = SERVER_DEFAULT_PORT;
        }
        self.name = self.name.trim().chars().take(60).collect();
        self.allowed_origins =
            self.allowed_origins.into_iter().map(|o| o.trim().trim_end_matches('/').to_owned()).filter(|o| o.starts_with("http://") || o.starts_with("https://")).collect();
        self.timeout_secs = self.timeout_secs.clamp(5, 120);
        self.concurrency = self.concurrency.clamp(1, 8);
        self.flaresolverr_url = self
            .flaresolverr_url
            .map(|u| u.trim().trim_end_matches('/').to_owned())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"));
        self.sniffer_timeout_secs = self.sniffer_timeout_secs.clamp(SNIFF_MIN_SECS, SNIFF_MAX_SECS);
        self
    }
}

/// `Backend::server_status` (Server page header, Settings > Server, title-bar chip).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ServerStatusVm {
    pub running: bool,
    pub port: u16,
    /// `http://<lan ip>:<port>` for every usable interface (what phones type or scan).
    pub urls: Vec<String>,
    pub name: String,
    /// Stable id of this server (`/health` `serverId`), once set up.
    pub server_id: Option<String>,
    /// Paired devices (viewer tokens).
    pub devices: u32,
    pub pairing_required: bool,
    pub viewer_password_set: bool,
    /// Enabled extensions shared with the phones.
    pub sources: u32,
    pub started_unix: Option<i64>,
    /// Why it is not running (`ErrorKind::AddrInUse` when the port is taken).
    pub error: Option<UiError>,
}

/// `Backend::server_open_pairing`: one-time 5-minute code (`POST /auth/pairing` of the classic server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPairingVm {
    pub code: String,
    pub expires_in_secs: u32,
    /// `tatakai://extensionserver/connect?server=<url>&code=<code>` (the QR payload).
    pub deep_link: String,
    /// SVG of the QR (`tk_ui::qr`).
    pub qr_svg: String,
}

/// One device token of the server (`GET /devices`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDeviceVm {
    pub id: String,
    pub label: String,
    /// `viewer` / `admin`.
    pub role: String,
    pub created_unix: i64,
    pub last_seen_unix: Option<i64>,
    /// The token the app itself uses (never revocable from the list).
    pub current: bool,
}

/// One server log line (`GET /logs`; classic dashboard "Logs" tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLogVm {
    pub id: u64,
    pub at_unix: i64,
    /// `request`, `source_completed`, `source_failed`, `pairing`, ...
    pub event: String,
    pub route: Option<String>,
    /// Extension / source name.
    pub name: Option<String>,
    pub count: Option<u32>,
    pub duration_ms: Option<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerLogPageVm {
    pub entries: Vec<ServerLogVm>,
    /// Pass back as `after` to get only newer lines.
    pub cursor: u64,
}

/// `Backend::flaresolverr_test`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FlareSolverrVm {
    pub ok: bool,
    pub version: Option<String>,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

/// `Backend::server_proxy_status` (media proxy + cache of the server).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProxyStatusVm {
    pub enabled: bool,
    /// `http://127.0.0.1:<port>/api/server/v1/media/` while running.
    pub base_url: Option<String>,
    pub cache_entries: u32,
    pub cache_bytes: u64,
    pub max_cache_bytes: u64,
}

/// `Backend::sniff_embed` input: an embed page an extension returned (`SourceVm::embed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffRequestVm {
    pub url: String,
    pub referer: Option<String>,
    /// Extension that produced the embed (its `browser_domains` / `network:domain:*` gate the hidden view).
    pub ext_id: Option<String>,
    /// `5..=60`.
    pub timeout_secs: u8,
}

/// `Backend::sniff_embed` result: the media URLs the page requested (`.m3u8` first, then `.mp4`), with the headers
/// mpv needs (Referer, User-Agent, cookies).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SniffResultVm {
    pub streams: Vec<StreamVm>,
    pub page_title: Option<String>,
    pub elapsed_ms: u32,
}

/// A command a paired phone sent over Tatakai Link to drive this PC's player (`link.remote-control`,
/// `link.handoff`; `tk-link::control::ControlCommand` is the wire form, `Backend::wait_remote_command` hands them to
/// the shell). `Open` = hand-off: open the title at the phone's position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteCommandVm {
    Play,
    Pause,
    TogglePause,
    NextEpisode,
    PrevEpisode,
    /// Absolute position, seconds.
    Seek(u32),
    Open { media_id: String, episode: u32, position_secs: u32 },
}

/// `Backend::send_to_device` (`link.send-to-device`): hand the current title / episode / position to a paired phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendToDeviceVm {
    pub device_id: String,
    pub media_id: String,
    pub episode: u32,
    pub position_secs: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = ServerPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn privileged_port_falls_back_and_urls_are_filtered() {
        let p = ServerPrefs {
            port: 80,
            allowed_origins: vec!["http://a/".into(), "ftp://b".into()],
            flaresolverr_url: Some("nope".into()),
            sniffer_timeout_secs: 0,
            ..ServerPrefs::default()
        }
        .sanitized();
        assert_eq!(p.port, SERVER_DEFAULT_PORT);
        assert_eq!(p.allowed_origins, vec!["http://a".to_owned()]);
        assert_eq!(p.flaresolverr_url, None);
        assert_eq!(p.sniffer_timeout_secs, SNIFF_MIN_SECS);
    }
}
