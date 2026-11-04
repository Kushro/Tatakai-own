//! Serde-friendly data types of the engine API (docs/redesign/10 section 2). All camelCase on the wire, like tk-core.
//! Signatures are final for wave 3; the engine (A) fills them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Lowercase hex v1 info-hash (40 chars). Stable across restarts, so it is also the id the UI and the library use.
pub type TorrentId = String;

/// Session-wide speed limits in bytes per second; `None` = unlimited.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    pub download_bps: Option<u32>,
    pub upload_bps: Option<u32>,
}

/// What [`crate::TorrentEngine::start`] needs. Built by tk-app from `TorrentSettingsVm` (`live/torrent.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EngineConfig {
    /// Where torrent data is written (one sub-folder per multi-file torrent).
    pub download_dir: PathBuf,
    /// Session persistence (librqbit JSON session + fastresume) and the engine's own side files (labels).
    pub persistence_dir: PathBuf,
    /// BitTorrent TCP/uTP listen port; 0 = pick a free one.
    pub listen_port: u16,
    /// Map the listen port on the router (UPnP); reported as `ProtocolStatus::nat` / `forwarding`.
    pub upnp: bool,
    /// Loopback HTTP streaming server port (always bound to 127.0.0.1); 0 = pick a free one.
    pub http_port: u16,
    pub dht: bool,
    /// Peer exchange (ut_pex). Best effort: if librqbit cannot turn it off, the flag is reported as it really is.
    pub pex: bool,
    /// Local service discovery (multicast on the LAN).
    pub lsd: bool,
    /// Remember torrents across restarts (librqbit session persistence + fastresume).
    pub persist_session: bool,
    /// Max connected peers per torrent.
    pub max_peers: u32,
    pub limits: Limits,
    /// Extra trackers announced for every torrent (public anime trackers by default: empty = only the torrent's own).
    pub trackers: Vec<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::new(),
            persistence_dir: PathBuf::new(),
            listen_port: 0,
            upnp: true,
            http_port: 0,
            dht: true,
            pex: true,
            lsd: true,
            persist_session: true,
            max_peers: 50,
            limits: Limits::default(),
            trackers: Vec::new(),
        }
    }
}

impl EngineConfig {
    /// Defaults with these two directories.
    pub fn new(download_dir: impl Into<PathBuf>, persistence_dir: impl Into<PathBuf>) -> Self {
        Self { download_dir: download_dir.into(), persistence_dir: persistence_dir.into(), ..Self::default() }
    }
}

/// What to add: a magnet link, a bare info-hash (resolved through DHT / trackers), the bytes of a `.torrent`, or an
/// http(s) URL of a `.torrent` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "camelCase")]
pub enum AddSource {
    Magnet(String),
    InfoHash(String),
    TorrentBytes(Vec<u8>),
    Url(String),
}

impl AddSource {
    /// `magnet:?...` -> `Magnet`, `http(s)://...` -> `Url`, 40 hex chars -> `InfoHash` (lowercased). Anything else
    /// is `None` (base32 hashes come inside magnets).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("magnet:?") {
            return Some(Self::Magnet(s.to_owned()));
        }
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return Some(Self::Url(s.to_owned()));
        }
        if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(Self::InfoHash(lower));
        }
        None
    }
}

/// App-level metadata stored by the engine next to its session (`<persistence_dir>/labels.json`) so the Library can
/// show series / episode without asking tk-app. Opaque to the engine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TorrentLabel {
    /// `MediaVm::id` (AniList id as text).
    pub media_id: String,
    /// Series title, display-ready.
    pub title: String,
    /// Episode currently / last watched from this torrent (batches change it with `set_label`).
    pub episode: Option<u32>,
    pub poster: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddOptions {
    /// Only download these file indices (`TorrentFile::index`); `None` = every file. Streaming a file adds it.
    pub only_files: Option<Vec<usize>>,
    /// Add without starting.
    pub paused: bool,
    /// Keep the torrent (library + data on disk) after playback; `false` = the app removes it with its files when the
    /// session ends (Settings > after watching).
    pub persist: bool,
    pub label: Option<TorrentLabel>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TorrentState {
    /// Resolving metadata (magnet) or checking existing data.
    Initializing,
    Downloading,
    /// Every selected file is complete (still uploading unless paused).
    Seeding,
    Paused,
    Error,
}

/// One file inside a torrent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentFile {
    /// librqbit file id (position in the torrent's file list).
    pub index: usize,
    /// Path inside the torrent, `/`-separated (`Season 1/[Group] Show - 01.mkv`).
    pub path: String,
    /// Last path component.
    pub name: String,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    /// 0.0..=1.0
    pub progress: f64,
    /// Wanted (in `only_files`, or every file when there is no selection).
    pub selected: bool,
    /// Open HTTP streams reading this file right now.
    pub streams: u32,
}

/// Protocol / feature status pills of the Overview (Hayase style). `true` = active right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProtocolStatus {
    pub dht: bool,
    pub lsd: bool,
    pub pex: bool,
    /// UPnP enabled and a gateway answered.
    pub nat: bool,
    /// The listen port is mapped (incoming connections possible).
    pub forwarding: bool,
    /// This torrent is kept after watching (`AddOptions::persist`) and the session is persisted.
    pub persisting: bool,
    /// At least one HTTP stream is open on this torrent.
    pub streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentStats {
    pub id: TorrentId,
    pub name: String,
    pub state: TorrentState,
    /// Engine message when `state == Error`.
    pub error: Option<String>,
    /// 0.0..=1.0 of the selected files.
    pub progress: f64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// Bytes of the selected files.
    pub selected_bytes: u64,
    /// Bytes of the whole torrent.
    pub size_bytes: u64,
    pub pieces: u32,
    pub piece_bytes: u64,
    pub pieces_have: u32,
    pub down_bps: u64,
    pub up_bps: u64,
    /// Connected peers that have every piece / that do not.
    pub seeders: u32,
    pub leechers: u32,
    /// Live connections ("wires").
    pub peers_connected: u32,
    /// Every peer address known (trackers, DHT, PEX), connected or not.
    pub peers_known: u32,
    /// `None` when unknown or finished.
    pub eta_secs: Option<u64>,
    /// Same as `protocol.streaming`: the UI shows "Streaming" instead of the ETA.
    pub streaming: bool,
    /// Since the torrent was added in this run.
    pub elapsed_secs: u64,
    pub protocol: ProtocolStatus,
}

/// One connected peer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    /// `ip:port` (`[v6]:port`).
    pub addr: String,
    /// Client from the peer id / extended handshake ("qBittorrent 4.6.2"), when known.
    pub client: Option<String>,
    /// 0.0..=1.0 of the torrent this peer has.
    pub progress: f64,
    pub down_bps: u64,
    pub up_bps: u64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// ISO 3166-1 alpha-2 when a lookup is available; `None` otherwise (no GeoIP database is shipped).
    pub country: Option<String>,
    /// qBittorrent-style letters, in this order when they apply: `D` downloading from it, `U` uploading to it,
    /// `I` incoming connection, `P` uTP, `E` encrypted, `X` learned through PEX, `H` through DHT, `S` seed.
    pub flags: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackerStatus {
    /// Last announce succeeded.
    Working,
    /// Announce in flight.
    Updating,
    /// Last announce failed (`TrackerInfo::message`).
    Failed,
    /// Not contacted yet (or the engine cannot tell).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerInfo {
    pub url: String,
    pub status: TrackerStatus,
    pub message: Option<String>,
    /// Scrape "downloaded" (completed) count.
    pub downloaded: Option<u32>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
}

/// One torrent of the session (Library and Downloads screens).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub id: TorrentId,
    pub name: String,
    pub size_bytes: u64,
    pub files: u32,
    /// 0.0..=1.0 of the selected files.
    pub progress: f64,
    pub state: TorrentState,
    /// Unix seconds when it was first added (kept across restarts).
    pub added_unix: i64,
    pub persist: bool,
    /// Folder holding the data (`open folder`).
    pub save_path: PathBuf,
    pub label: Option<TorrentLabel>,
}

/// Engine notifications (`TorrentEngine::events`). The UI polls stats; events are for tk-app policies
/// (after-watch deletion, background downloads, logs).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TorrentEvent {
    Added { id: TorrentId },
    /// Metadata known (magnets): `files()` works from here on.
    MetadataReady { id: TorrentId },
    StateChanged { id: TorrentId, state: TorrentState },
    FileCompleted { id: TorrentId, index: usize },
    /// Every selected file is complete.
    Completed { id: TorrentId },
    StreamOpened { id: TorrentId, index: usize },
    StreamClosed { id: TorrentId, index: usize },
    Removed { id: TorrentId },
    Error { id: TorrentId, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_source_parse() {
        let hash = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
        assert_eq!(AddSource::parse(hash), Some(AddSource::InfoHash(hash.to_ascii_lowercase())));
        assert!(matches!(AddSource::parse(" magnet:?xt=urn:btih:abc "), Some(AddSource::Magnet(m)) if m == "magnet:?xt=urn:btih:abc"));
        assert!(matches!(AddSource::parse("https://nyaa.si/download/1.torrent"), Some(AddSource::Url(_))));
        assert_eq!(AddSource::parse("not a torrent"), None);
        assert_eq!(AddSource::parse(&hash[1..]), None);
    }
}
