//! The seam between the UI and the app. tk-ui only knows these view-model types and the [`Backend`]
//! trait; tk-app implements it over tk-core / tk-ext / tk-link. Strings are display-ready except
//! where an i18n key is noted.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::model::{CompatLevel, Pairing, Settings, UiError};

/// A poster card. `id` is the AniList id as text (`"mal:<id>"` when only MAL is known).
#[derive(Debug, Clone, PartialEq)]
pub struct MediaVm {
    pub id: String,
    pub title: String,
    pub cover: Option<String>,
    /// Poster dominant color from the metadata provider (`#RRGGBB`), used for the cover placeholder.
    pub color: Option<String>,
    /// `2024 - 12 ep` style line, already formatted.
    pub subtitle: String,
    pub score: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContinueVm {
    pub media: MediaVm,
    pub episode: u32,
    pub progress_secs: u32,
    pub duration_secs: Option<u32>,
}

impl ContinueVm {
    pub fn fraction(&self) -> f64 {
        match self.duration_secs {
            Some(d) if d > 0 => (f64::from(self.progress_secs) / f64::from(d)).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// One home row. `title_key` is an i18n key (`feed.trending`, ...); a failed row carries its error
/// instead of items so one provider failure never breaks the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct SectionVm {
    pub title_key: String,
    pub items: Result<Vec<MediaVm>, UiError>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct HomeVm {
    pub continue_watching: Vec<ContinueVm>,
    pub sections: Vec<SectionVm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DetailsVm {
    pub media: MediaVm,
    pub banner: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub episodes_total: Option<u32>,
    /// Last watched episode for this anime (0 = none), to offer "Continue".
    pub resume_episode: u32,
    pub resume_secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeVm {
    pub number: u32,
    pub title: Option<String>,
    pub image: Option<String>,
    pub air_date: Option<String>,
    pub is_filler: bool,
    /// 0.0..=1.0 from the local history.
    pub watched: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubtitleVm {
    pub url: String,
    pub lang: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamVm {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub provider: String,
    pub quality: Option<String>,
    pub subtitles: Vec<SubtitleVm>,
}

/// What the player needs to start an episode.
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackVm {
    pub title: String,
    pub poster: Option<String>,
    pub episodes_total: Option<u32>,
    /// Sources in the order to try (the UI falls back to the next one on failure, max 3).
    pub streams: Vec<StreamVm>,
    pub start_secs: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionVm {
    pub id: String,
    pub name: String,
    pub version: String,
    pub compat: CompatLevel,
    pub enabled: bool,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceVm {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// Unix seconds, `None` if never seen after pairing.
    pub last_seen_unix: Option<i64>,
    /// Has a live session with this PC right now ("Connected now" pill).
    pub online: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccountVm {
    pub email: Option<String>,
    /// False when the build/config has no Supabase URL + anon key: the account section explains it.
    pub configured: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkStatusVm {
    pub running: bool,
    pub port: u16,
    pub devices: Vec<DeviceVm>,
    /// Why the server is not running (`running == false`). `ErrorKind::AddrInUse` = `port` is taken
    /// (os error 10048 / 98); `detail` carries the OS message for the collapsible block.
    pub error: Option<UiError>,
}

// ---------------------------------------------------------------------------------------------
// Wave 1 contract (docs/redesign/08): sources, library, extension details, Link status
// ---------------------------------------------------------------------------------------------

/// Mirrors `tk_core::sources::SourceKind`. Persisted inside `Settings` (`"web"` / `"torrent"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKindVm {
    Web,
    Torrent,
}

impl SourceKindVm {
    pub const ALL: [SourceKindVm; 2] = [Self::Web, Self::Torrent];

    /// `web` / `torrent`: i18n suffix (`src.<id>`), `data-kind` value and middle part of `SourceVm::id`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Torrent => "torrent",
        }
    }
}

/// One ranked source of an episode (`Backend::sources` returns them best first).
#[derive(Debug, Clone, PartialEq)]
pub struct SourceVm {
    /// `tk_core::sources::source_key`: `"<ext_id>|<kind>|<label>"`. Pass it back to `playback_source`,
    /// `report_source_result` and `set_picked_source`.
    pub id: String,
    pub ext_id: String,
    pub ext_name: String,
    pub kind: SourceKindVm,
    /// Server ("Alpha") for web, release group ("SubsPlease") for torrent.
    pub label: String,
    /// `"1080p"`; empty when unknown.
    pub quality: String,
    /// "HLS", "MP4", "HEVC 10-bit" ...
    pub container: Option<String>,
    /// ISO 639-1 lowercase (`es`); the UI upper-cases them.
    pub subs: Vec<String>,
    pub dubs: Vec<String>,
    /// 0.0..=1.0. Tones: >= 0.9 ok, >= 0.7 warn, else err (`src.rel.high|mid|low`).
    pub reliability: f32,
    pub note: Option<String>,
    /// Web only.
    pub latency_ms: Option<u32>,
    /// Torrent only.
    pub size_bytes: Option<u64>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
    pub info_hash: Option<String>,
    /// False when the extension is disabled or unsupported: the row is shown disabled (`src.ext_off`).
    pub ext_enabled: bool,
    pub score: f32,
    /// The first entry of the ranked list (the one Auto mode plays).
    pub recommended: bool,
    /// The user's manual pick for this episode (`set_picked_source`); at most one per list.
    pub picked: bool,
    /// Release tags for the picker chips / filters (wave 3, docs/redesign/10 section 3); default for web sources.
    pub tags: ReleaseTagsVm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoryItemVm {
    pub media: MediaVm,
    pub episode: u32,
    pub watched_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LibraryVm {
    pub continue_watching: Vec<ContinueVm>,
    /// Newest first, one row per watched episode.
    pub history: Vec<HistoryItemVm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtLogLevel {
    Ok,
    Warn,
    Err,
}

/// One line of an extension's log ("search", "\"dandadan\" -> 3 results in 412 ms").
#[derive(Debug, Clone, PartialEq)]
pub struct ExtLogVm {
    pub level: ExtLogLevel,
    /// Operation: `install`, `search`, `stream`, `fetch`, `compat`, `session` ...
    pub op: String,
    pub message: String,
    pub at_unix: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionDetailVm {
    pub id: String,
    /// Hosts from the `network:domain:*` permissions.
    pub domains: Vec<String>,
    /// Package hash, display form (`9f2c…b71e`).
    pub hash: String,
    pub updated_unix: Option<i64>,
    /// Manifest `type` (`anime`, `torrent` ...).
    pub kind: String,
}

/// What this PC offers to paired phones (`GET /v1/capabilities`, measured locally).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LinkCapsVm {
    pub gpu: Option<String>,
    pub vulkan: Option<String>,
    pub encoder: Option<String>,
    pub upscale: Option<String>,
    /// `(profile id, supported)`; `None` = not measured yet (`link.caps.unknown`).
    pub profiles: Vec<(String, Option<bool>)>,
    /// Certificate fingerprint, display form (`7A3F 91C2 ...`).
    pub fingerprint: String,
    pub lan_ip: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Wave 2 contract (docs/redesign/09): player modes, Linux-first texts
// ---------------------------------------------------------------------------------------------

/// The vendored mpv-anime-build release behind the "Embedded enhanced" player mode.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AnimeBuildVm {
    /// The pinned release zip ships with this installation (next to the executable, or the source tree in development).
    pub available: bool,
    /// Release version (`"5.6"`), display-ready; empty when not available.
    pub version: String,
    /// Already verified and extracted into the data directory (the first use does not have to wait).
    pub extracted: bool,
    /// The extracted config directory (display form), when `extracted`.
    pub path: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Wave 3 contract (docs/redesign/10): torrents (engine, client screens, player), release tags
// ---------------------------------------------------------------------------------------------

/// Mirrors `tk_core::release::ReleaseSource`. Chip text is `label()` (technical code, not translated).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseSourceVm {
    Bd,
    Remux,
    Web,
    Tv,
    Dvd,
}

impl ReleaseSourceVm {
    pub const ALL: [ReleaseSourceVm; 5] = [Self::Bd, Self::Remux, Self::Web, Self::Tv, Self::Dvd];

    /// `bd` / `remux` / `web` / `tv` / `dvd`: filter ids and `data-*` values.
    pub fn id(self) -> &'static str {
        match self {
            Self::Bd => "bd",
            Self::Remux => "remux",
            Self::Web => "web",
            Self::Tv => "tv",
            Self::Dvd => "dvd",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bd => "BD",
            Self::Remux => "REMUX",
            Self::Web => "WEB",
            Self::Tv => "TV",
            Self::Dvd => "DVD",
        }
    }
}

/// Mirrors `tk_core::release::VideoCodec`. Chip text is `label()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VideoCodecVm {
    H264,
    Hevc,
    Av1,
    Vp9,
}

impl VideoCodecVm {
    pub const ALL: [VideoCodecVm; 4] = [Self::H264, Self::Hevc, Self::Av1, Self::Vp9];

    pub fn id(self) -> &'static str {
        match self {
            Self::H264 => "h264",
            Self::Hevc => "hevc",
            Self::Av1 => "av1",
            Self::Vp9 => "vp9",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::H264 => "H.264",
            Self::Hevc => "HEVC",
            Self::Av1 => "AV1",
            Self::Vp9 => "VP9",
        }
    }
}

/// Release tags of a source (`SourceVm::tags`), mapped from `tk_core::release::ReleaseTags`. Resolution is
/// `SourceVm::quality`. Empty (default) for web sources.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReleaseTagsVm {
    pub group: Option<String>,
    pub source: Option<ReleaseSourceVm>,
    pub codec: Option<VideoCodecVm>,
    pub bit_depth: Option<u8>,
    pub dual_audio: bool,
    pub multi_sub: bool,
    /// Season pack / batch (one torrent, many episodes).
    pub batch: bool,
    /// SeaDex "best" release.
    pub seadex_best: bool,
    /// Episode range the release covers (`(12, 12)` for a single episode).
    pub episodes: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TorrentStateVm {
    Initializing,
    Downloading,
    Seeding,
    Paused,
    Error,
}

impl TorrentStateVm {
    pub const ALL: [TorrentStateVm; 5] = [Self::Initializing, Self::Downloading, Self::Seeding, Self::Paused, Self::Error];

    /// i18n suffix (`torrent.state.<id>`) and `data-state` value of the state pill.
    pub fn id(self) -> &'static str {
        match self {
            Self::Initializing => "initializing",
            Self::Downloading => "downloading",
            Self::Seeding => "seeding",
            Self::Paused => "paused",
            Self::Error => "error",
        }
    }
}

/// Protocol status pills of the Overview (`torrent.proto.<field>`). `true` = active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TorrentProtocolVm {
    pub dht: bool,
    pub lsd: bool,
    pub pex: bool,
    pub nat: bool,
    pub forwarding: bool,
    pub persisting: bool,
    pub streaming: bool,
}

/// Overview of one torrent (polled about once a second while a torrent screen or the player overlay shows).
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentStatsVm {
    /// Lowercase hex info-hash; the id every `torrent_*` call takes.
    pub id: String,
    pub name: String,
    pub info_hash: String,
    pub state: TorrentStateVm,
    pub error: Option<String>,
    /// 0.0..=1.0 of the selected files.
    pub progress: f64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// Selected files.
    pub total_bytes: u64,
    /// Whole torrent.
    pub size_bytes: u64,
    pub pieces: u32,
    pub piece_bytes: u64,
    pub pieces_have: u32,
    pub down_bps: u64,
    pub up_bps: u64,
    pub seeders: u32,
    pub leechers: u32,
    /// Live connections ("wires").
    pub peers: u32,
    pub eta_secs: Option<u64>,
    /// Shows "Streaming" instead of the remaining time.
    pub streaming: bool,
    pub elapsed_secs: u64,
    pub protocol: TorrentProtocolVm,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TorrentFileVm {
    pub index: usize,
    /// Path inside the torrent, `/`-separated.
    pub path: String,
    pub name: String,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    /// 0.0..=1.0
    pub progress: f64,
    pub selected: bool,
    /// Open streams right now.
    pub streams: u32,
    /// Episode parsed from the file name (`tk_core::release::parse_release`), for batches.
    pub episode: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TorrentPeerVm {
    /// `ip:port`
    pub addr: String,
    pub client: Option<String>,
    pub progress: f64,
    pub down_bps: u64,
    pub up_bps: u64,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    /// ISO 3166-1 alpha-2 (`JP`); `None` when unknown.
    pub country: Option<String>,
    /// qBittorrent-style letters (`D U I P E X H S`, see `tk_torrent::PeerInfo::flags`); legend keys
    /// `torrent.peers.flag.<letter lowercase>`.
    pub flags: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackerStatusVm {
    Working,
    Updating,
    Failed,
    Unknown,
}

impl TrackerStatusVm {
    pub const ALL: [TrackerStatusVm; 4] = [Self::Working, Self::Updating, Self::Failed, Self::Unknown];

    /// i18n suffix (`torrent.tracker.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Updating => "updating",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TorrentTrackerVm {
    pub url: String,
    pub status: TrackerStatusVm,
    pub message: Option<String>,
    pub downloaded: Option<u32>,
    pub seeders: Option<u32>,
    pub leechers: Option<u32>,
}

/// One saved torrent (Library) or background download (Downloads).
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentLibraryItemVm {
    pub id: String,
    pub name: String,
    /// `MediaVm::id` of the series, when it was added from the app.
    pub media_id: Option<String>,
    /// Series title.
    pub series: Option<String>,
    pub episode: Option<u32>,
    pub poster: Option<String>,
    pub files: u32,
    pub size_bytes: u64,
    pub progress: f64,
    pub state: TorrentStateVm,
    pub added_unix: i64,
    /// Kept after watching (otherwise removed with its files when the session ends).
    pub persist: bool,
    /// Data folder, display form.
    pub path: String,
}

/// What to do with a torrent after watching (Settings of the torrent client).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AfterWatchVm {
    /// Keep it in the Library (and seeding while the app runs).
    Keep,
    /// Remove it and its files when the player closes.
    #[default]
    Delete,
}

impl AfterWatchVm {
    pub const ALL: [AfterWatchVm; 2] = [Self::Keep, Self::Delete];

    /// i18n suffix (`torrent.settings.after_watch.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Delete => "delete",
        }
    }
}

/// Settings of the torrent client, persisted by tk-app in `<data>/torrent-settings.json` (not inside `Settings`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TorrentSettingsVm {
    /// Download folder; empty = `default_download_dir`.
    pub download_dir: String,
    /// Filled by the backend on load (display form); never persisted.
    #[serde(skip)]
    pub default_download_dir: String,
    /// KiB/s, 0 = unlimited.
    pub download_limit_kib: u32,
    pub upload_limit_kib: u32,
    /// BitTorrent listen port; 0 = random.
    pub listen_port: u16,
    /// UPnP port forwarding.
    pub upnp: bool,
    /// Max peers per torrent.
    pub max_peers: u32,
    pub dht: bool,
    pub pex: bool,
    pub lsd: bool,
    pub after_watch: AfterWatchVm,
    /// Background downloads (keep downloading / seeding saved torrents while nothing plays). Only honoured when
    /// `background_ack` is true.
    pub background_downloads: bool,
    /// The user accepted the CPU / network usage warning ("I understand").
    pub background_ack: bool,
}

/// Bounds of `TorrentSettingsVm::sanitized`.
pub const TORRENT_MAX_PEERS: u32 = 500;
pub const TORRENT_MAX_LIMIT_KIB: u32 = 1_000_000;

impl Default for TorrentSettingsVm {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
            default_download_dir: String::new(),
            download_limit_kib: 0,
            upload_limit_kib: 0,
            listen_port: 0,
            upnp: true,
            max_peers: 50,
            dht: true,
            pex: true,
            lsd: true,
            after_watch: AfterWatchVm::Delete,
            background_downloads: false,
            background_ack: false,
        }
    }
}

impl TorrentSettingsVm {
    /// Clamp numbers into range (`max_peers` 1..=500, limits <= 1 000 000 KiB/s), trim the folder, and turn
    /// background downloads off when the warning was never accepted.
    pub fn sanitized(mut self) -> Self {
        self.download_dir = self.download_dir.trim().to_owned();
        self.max_peers = self.max_peers.clamp(1, TORRENT_MAX_PEERS);
        self.download_limit_kib = self.download_limit_kib.min(TORRENT_MAX_LIMIT_KIB);
        self.upload_limit_kib = self.upload_limit_kib.min(TORRENT_MAX_LIMIT_KIB);
        if !self.background_ack {
            self.background_downloads = false;
        }
        self
    }
}

/// A torrent playback session: what `PlayerPage` plays plus what it needs for the torrent overlay, the seek-bar
/// marks and "next episode" in a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentPlaybackVm {
    /// `streams` has exactly one entry: the loopback HTTP URL of the episode file.
    pub playback: PlaybackVm,
    pub torrent_id: String,
    pub file_index: usize,
    pub file_name: String,
    /// The torrent has more than one video file: the next episode can reuse it.
    pub batch: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SkipKindVm {
    Intro,
    Outro,
    Recap,
}

impl SkipKindVm {
    pub const ALL: [SkipKindVm; 3] = [Self::Intro, Self::Outro, Self::Recap];

    /// i18n suffix (`player.skip.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Intro => "intro",
            Self::Outro => "outro",
            Self::Recap => "recap",
        }
    }
}

/// An intro / outro / recap interval of an episode (seek-bar marks, "Skip intro").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkipMarkVm {
    pub kind: SkipKindVm,
    pub start_secs: f64,
    pub end_secs: f64,
}

pub type UiResult<T> = Result<T, UiError>;

/// Implemented by tk-app. All methods are cancel-safe; none blocks the UI thread for long.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    // --- catalogue ---
    async fn home(&self) -> UiResult<HomeVm>;
    async fn search(&self, query: &str, page: u32) -> UiResult<Vec<MediaVm>>;
    async fn details(&self, id: &str) -> UiResult<DetailsVm>;
    async fn episodes(&self, id: &str) -> UiResult<Vec<EpisodeVm>>;
    /// Resolve playable streams through the installed extensions.
    async fn playback(&self, id: &str, episode: u32) -> UiResult<PlaybackVm>;
    /// Local-first history write (+ Supabase sync when signed in).
    async fn record_progress(&self, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) -> UiResult<()>;

    // --- extensions ---
    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>>;
    async fn install_extension(&self, path: &str) -> UiResult<ExtensionVm>;
    async fn set_extension_enabled(&self, id: &str, enabled: bool) -> UiResult<()>;
    async fn remove_extension(&self, id: &str) -> UiResult<()>;

    // --- settings / account ---
    async fn load_settings(&self) -> UiResult<Settings>;
    async fn save_settings(&self, settings: &Settings) -> UiResult<()>;
    async fn account(&self) -> UiResult<AccountVm>;
    async fn sign_in(&self, email: &str, password: &str) -> UiResult<AccountVm>;
    async fn sign_out(&self) -> UiResult<()>;

    // --- Tatakai Link ---
    async fn link_status(&self) -> UiResult<LinkStatusVm>;
    async fn link_open_pairing(&self) -> UiResult<Pairing>;
    async fn link_revoke(&self, device_id: &str) -> UiResult<()>;
    /// Windows Firewall rule for the private profile (UAC prompt). Returns when the elevated process ends.
    async fn link_allow_private_network(&self) -> UiResult<()>;

    // --- wave 1 (docs/redesign/08). Defaults return `ErrorKind::NotWired` so every implementation keeps
    // compiling; tk-app (F) overrides them. ---

    /// All sources of an episode, ranked best first with the current `Settings::sources` prefs.
    async fn sources(&self, _id: &str, _episode: u32) -> UiResult<Vec<SourceVm>> {
        Err(UiError::not_wired("sources"))
    }
    /// Resolve one source (`SourceVm::id`) into a playable stream (`streams` has exactly one entry).
    /// A torrent source fails with `ErrorKind::TorrentM2`.
    async fn playback_source(&self, _id: &str, _episode: u32, _source_id: &str) -> UiResult<PlaybackVm> {
        Err(UiError::not_wired("playback_source"))
    }
    /// Feed the reliability average of that source (`ok` = playback started).
    async fn report_source_result(&self, _source_id: &str, _ok: bool) -> UiResult<()> {
        Err(UiError::not_wired("report_source_result"))
    }
    /// Remember (`Some`) or forget (`None`) the user's pick for this episode. The pick expires when the
    /// episode is completed.
    async fn set_picked_source(&self, _id: &str, _episode: u32, _source_id: Option<&str>) -> UiResult<()> {
        Err(UiError::not_wired("set_picked_source"))
    }
    async fn library(&self) -> UiResult<LibraryVm> {
        Err(UiError::not_wired("library"))
    }
    /// Push dirty history and pull remote rows now (signed in only; otherwise `ErrorKind::Auth`).
    async fn sync_now(&self) -> UiResult<()> {
        Err(UiError::not_wired("sync_now"))
    }
    /// Newest first.
    async fn extension_logs(&self, _id: &str) -> UiResult<Vec<ExtLogVm>> {
        Err(UiError::not_wired("extension_logs"))
    }
    async fn extension_detail(&self, _id: &str) -> UiResult<ExtensionDetailVm> {
        Err(UiError::not_wired("extension_detail"))
    }
    /// Stop the Link server and start it again on `port`. A taken port is not an `Err`: it returns a
    /// status with `running: false` and `error: Some(AddrInUse)`.
    async fn link_restart(&self, _port: u16) -> UiResult<LinkStatusVm> {
        Err(UiError::not_wired("link_restart"))
    }
    async fn link_rename_device(&self, _device_id: &str, _name: &str) -> UiResult<()> {
        Err(UiError::not_wired("link_rename_device"))
    }
    async fn link_capabilities(&self) -> UiResult<LinkCapsVm> {
        Err(UiError::not_wired("link_capabilities"))
    }

    // --- wave 2 (docs/redesign/09). Same rule: defaults keep every implementation compiling. ---

    /// Whether the "Embedded enhanced" mode can be used and whether its files are ready.
    async fn anime_build_status(&self) -> UiResult<AnimeBuildVm> {
        Err(UiError::not_wired("anime_build_status"))
    }
    /// Verify the pinned SHA-256 of the vendored release and extract it into the data directory if needed
    /// (idempotent; a later call returns at once). Returns the config directory for
    /// `tk_player::EngineMode::Enhanced { config_dir, .. }`. Can take a few seconds the first time.
    async fn prepare_anime_build(&self) -> UiResult<PathBuf> {
        Err(UiError::not_wired("prepare_anime_build"))
    }
    /// The real data directory for Settings texts (replaces the hard-coded `%APPDATA%`), e.g.
    /// `/home/ana/.config/Tatakai-Next` or `C:\Users\ana\AppData\Roaming\Tatakai-Next`. Empty when unknown.
    fn data_dir_display(&self) -> String {
        String::new()
    }

    // --- wave 3 (docs/redesign/10): torrents. Same rule: defaults keep every implementation compiling. ---

    /// Play `episode` of `id` from a torrent. `source` is a torrent `SourceVm::id` (`<ext>|torrent|<label>`), a
    /// magnet link, or a 40-hex info-hash / torrent id (an already added torrent is reused: next episode of a batch).
    /// Adds the torrent (engine started on demand), picks the episode file, returns its loopback stream URL.
    async fn torrent_play(&self, _source: &str, _id: &str, _episode: u32) -> UiResult<TorrentPlaybackVm> {
        Err(UiError::not_wired("torrent_play"))
    }
    /// Called when the player leaves a torrent session (`completed` = the episode was watched to the end). Applies
    /// `TorrentSettingsVm::after_watch` and the background-download policy.
    async fn torrent_playback_ended(&self, _torrent_id: &str, _completed: bool) -> UiResult<()> {
        Err(UiError::not_wired("torrent_playback_ended"))
    }
    /// Start a background download (same `source` forms as `torrent_play`; `episode: None` = every file). Needs
    /// background downloads enabled (`background_downloads_enabled`), else `ErrorKind::Other` with a detail.
    /// Returns the torrent id.
    async fn torrent_add_download(&self, _source: &str, _id: &str, _episode: Option<u32>) -> UiResult<String> {
        Err(UiError::not_wired("torrent_add_download"))
    }
    async fn torrent_stats(&self, _torrent_id: &str) -> UiResult<TorrentStatsVm> {
        Err(UiError::not_wired("torrent_stats"))
    }
    async fn torrent_files(&self, _torrent_id: &str) -> UiResult<Vec<TorrentFileVm>> {
        Err(UiError::not_wired("torrent_files"))
    }
    /// Fastest first.
    async fn torrent_peers(&self, _torrent_id: &str) -> UiResult<Vec<TorrentPeerVm>> {
        Err(UiError::not_wired("torrent_peers"))
    }
    async fn torrent_trackers(&self, _torrent_id: &str) -> UiResult<Vec<TorrentTrackerVm>> {
        Err(UiError::not_wired("torrent_trackers"))
    }
    /// Every torrent of the session (saved + in progress), newest first.
    async fn torrent_library(&self) -> UiResult<Vec<TorrentLibraryItemVm>> {
        Err(UiError::not_wired("torrent_library"))
    }
    async fn torrent_remove(&self, _torrent_id: &str, _delete_files: bool) -> UiResult<()> {
        Err(UiError::not_wired("torrent_remove"))
    }
    async fn torrent_pause(&self, _torrent_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_pause"))
    }
    async fn torrent_resume(&self, _torrent_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_resume"))
    }
    /// Keep (`true`) or stop keeping a torrent after watching (Library).
    async fn torrent_set_persist(&self, _torrent_id: &str, _persist: bool) -> UiResult<()> {
        Err(UiError::not_wired("torrent_set_persist"))
    }
    /// Open the data folder in the system file manager (`xdg-open` / Explorer).
    async fn torrent_open_folder(&self, _torrent_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_open_folder"))
    }
    /// With `default_download_dir` filled.
    async fn torrent_settings(&self) -> UiResult<TorrentSettingsVm> {
        Err(UiError::not_wired("torrent_settings"))
    }
    /// Persist (sanitized) and apply what can change live (limits); port / DHT changes restart the engine when idle.
    async fn save_torrent_settings(&self, _settings: &TorrentSettingsVm) -> UiResult<()> {
        Err(UiError::not_wired("save_torrent_settings"))
    }
    /// The user accepted the background-download warning ("I understand"); persisted (`background_ack`).
    async fn background_downloads_ack(&self) -> UiResult<()> {
        Err(UiError::not_wired("background_downloads_ack"))
    }
    /// `background_ack && background_downloads`.
    async fn background_downloads_enabled(&self) -> UiResult<bool> {
        Err(UiError::not_wired("background_downloads_enabled"))
    }
    /// Downloaded ranges of one file of a torrent as sorted, merged `(start, end)` fractions 0.0..=1.0 (byte
    /// fractions of the file; the seek bar draws them as time fractions).
    async fn seek_marks(&self, _torrent_id: &str, _file_index: usize) -> UiResult<Vec<(f64, f64)>> {
        Err(UiError::not_wired("seek_marks"))
    }
    /// Intro / outro / recap of an episode when a provider knows them (AniSkip by MAL id); empty when unknown.
    /// Works for web sources too.
    async fn skip_marks(&self, _id: &str, _episode: u32) -> UiResult<Vec<SkipMarkVm>> {
        Err(UiError::not_wired("skip_marks"))
    }
}

pub type BackendRef = Arc<dyn Backend>;
