//! The seam between the UI and the app. tk-ui only knows these view-model types and the [`Backend`]
//! trait; tk-app implements it over tk-core / tk-ext / tk-link. Strings are display-ready except
//! where an i18n key is noted.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::model::{CompatLevel, Pairing, PlatformVm, Settings, UiError};
pub use crate::model::ListServiceVm;

// Wave B (docs/redesign/16-parity-wave-b-contract.md section 3): the view-models of each owner live in
// `model/<area>.rs` next to its prefs and are re-exported here so `tk_ui::backend::X` keeps working for every VM.
pub use crate::model::downloads::{
    AutoDownloadRunVm, DownloadKindVm, DownloadRequestVm, DownloadStateVm, DownloadStorageVm, DownloadVm, LibraryScanVm,
    LocalItemVm,
};
pub use crate::model::ext::{
    CustomProviderVm, ExtCatalogItemVm, ExtPrefFieldVm, ExtPrefKindVm, ExtRepoVm, ExtServerPairVm, ExtServerVm, ExtSourceVm,
    ExtUpdateVm, ProviderItemVm, StremioAddonVm,
};
pub use crate::model::home::{
    AiringSlotVm, AtlasPointVm, BecauseVm, LatestEpisodeVm, MatureContentVm, ProviderHealthVm, RecommendationVm,
    RecsFeedbackVm, TasteProfileVm, TopRangeVm, TorrentActivityVm,
};
pub use crate::model::manga::{
    ChapterProviderVm, ChapterRefVm, ChapterVm, ContinueReadVm, MangaDetailsVm, PageVm, ReadHomeVm, ReadKindVm,
    ReadProgressVm,
};
pub use crate::model::novel::{
    KittenStatusVm, NovelContentVm, NovelDetailsVm, TtsClipVm, TtsProviderVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm,
};
pub use crate::model::server::{
    FlareSolverrVm, ProxyStatusVm, RemoteCommandVm, SendToDeviceVm, ServerDeviceVm, ServerLogPageVm, ServerLogVm,
    ServerPairingVm, ServerStatusVm, SniffRequestVm, SniffResultVm,
};
pub use crate::model::social::{
    ActivityFeedVm, ActivityKindVm, ActivityReplyVm, ActivityVm, NotificationPageVm, NotificationVm, ProfileStatsVm,
    ProfileVm, ReviewVm, UserPageVm, UserVm,
};

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
    /// Torrent only (wave A, worker i): whether a connected debrid service has this torrent cached. `None` = no
    /// debrid account or not checked. A cached torrent plays through `Backend::debrid_play` instead of the engine.
    pub debrid: Option<DebridStatusVm>,
    /// Web only (wave B, worker k; plan 14 section 0.1 #13): the URL is an embed page, not a media URL. Playing it
    /// goes through the hidden web view sniffer (`Backend::sniff_embed`), which costs a few seconds, so
    /// `tk_core::sources::score` ranks it below direct streams. The picker shows an "Embed" chip (`data-embed`).
    pub embed: bool,
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
    // --- wave A (worker i): manual torrents, trackers, schedule, cache, preload ---
    /// Extra tracker URLs announced for every torrent (classic "custom trackers").
    pub trackers: Vec<String>,
    /// Bandwidth schedule: alternative limits during a daily window.
    pub schedule: BandwidthScheduleVm,
    /// Disk cap for torrents that are not kept (GiB); 0 = no cap. The oldest non-persisted torrents go first.
    pub cache_limit_gib: u32,
    /// Start downloading the next episode of a batch (or the next episode's best torrent) while one plays.
    pub preload_next: bool,
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
            trackers: Vec::new(),
            schedule: BandwidthScheduleVm::default(),
            cache_limit_gib: 0,
            preload_next: false,
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
        self.trackers = self
            .trackers
            .into_iter()
            .map(|t| t.trim().to_owned())
            .filter(|t| t.starts_with("udp://") || t.starts_with("http://") || t.starts_with("https://") || t.starts_with("wss://"))
            .collect();
        self.schedule = self.schedule.sanitized();
        self
    }
}

/// Daily window with its own limits (Torrents > Settings > Schedule; classic "bandwidth schedule"). Hours are
/// local, `0..=23`; a window that ends before it starts crosses midnight (`22` -> `7`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BandwidthScheduleVm {
    pub enabled: bool,
    pub start_hour: u8,
    pub end_hour: u8,
    /// KiB/s inside the window, 0 = unlimited.
    pub download_limit_kib: u32,
    pub upload_limit_kib: u32,
}

impl Default for BandwidthScheduleVm {
    fn default() -> Self {
        Self { enabled: false, start_hour: 0, end_hour: 7, download_limit_kib: 0, upload_limit_kib: 0 }
    }
}

impl BandwidthScheduleVm {
    pub fn sanitized(mut self) -> Self {
        self.start_hour = self.start_hour.min(23);
        self.end_hour = self.end_hour.min(23);
        self.download_limit_kib = self.download_limit_kib.min(TORRENT_MAX_LIMIT_KIB);
        self.upload_limit_kib = self.upload_limit_kib.min(TORRENT_MAX_LIMIT_KIB);
        self
    }

    /// Whether `hour` (local, 0..=23) falls inside the window.
    pub fn active_at(&self, hour: u8) -> bool {
        if !self.enabled {
            return false;
        }
        if self.start_hour <= self.end_hour {
            (self.start_hour..self.end_hour).contains(&hour)
        } else {
            hour >= self.start_hour || hour < self.end_hour
        }
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

// ---------------------------------------------------------------------------------------------
// Wave A contract (docs/redesign/15-parity-wave-a-contract.md section 3). View-models per owner: a (search,
// character), c (details), d (subtitles / translation / capture), e (presence), f (system / wizard / reset), g (lists),
// i (manual torrents / debrid). Frozen after the contract: workers map to these, never change them.
// ---------------------------------------------------------------------------------------------

/// Sort orders of the search (worker a). `id()` is the i18n suffix `search.sort.<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchSortVm {
    /// Text relevance when there is a query, popularity otherwise.
    #[default]
    Relevance,
    Popularity,
    Score,
    Trending,
    Newest,
    Title,
}

impl SearchSortVm {
    pub const ALL: [SearchSortVm; 6] = [Self::Relevance, Self::Popularity, Self::Score, Self::Trending, Self::Newest, Self::Title];

    pub fn id(self) -> &'static str {
        match self {
            Self::Relevance => "relevance",
            Self::Popularity => "popularity",
            Self::Score => "score",
            Self::Trending => "trending",
            Self::Newest => "newest",
            Self::Title => "title",
        }
    }
}

/// A filtered, paginated search (worker a; classic `SearchPage` anime filters). Every filter is optional; the
/// backend turns them into AniList `Page(media(...))` variables (`tk_core::metadata::anilist::search`). Values are
/// AniList's own ids where it has them: genres as shown (`"Action"`), tags by name, `format` in
/// `TV | TV_SHORT | MOVIE | OVA | ONA | SPECIAL | MUSIC`, `status` in `releasing | finished | upcoming | cancelled |
/// hiatus`, `season` in `winter | spring | summer | fall`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchQueryVm {
    pub query: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    /// Exact season year (with `season`) or the only year when the range is unused.
    pub year: Option<i32>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub episodes_min: Option<u32>,
    pub episodes_max: Option<u32>,
    /// Average score 0..=100.
    pub score_min: Option<u32>,
    pub score_max: Option<u32>,
    pub sort: SearchSortVm,
    /// Include 18+ titles (off by default).
    pub adult: bool,
    /// 1-based.
    pub page: u32,
}

/// One page of results (worker a): search, feeds ("See all"), studio pages, list pages.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SearchPageVm {
    pub items: Vec<MediaVm>,
    pub page: u32,
    pub has_next: bool,
    /// Total results when the provider says (AniList `pageInfo.total`).
    pub total: Option<u32>,
}

/// What `Backend::search_by_image` receives (worker a; trace.moe).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageQueryVm {
    /// A public image URL (trace.moe fetches it).
    Url(String),
    /// Image bytes picked locally with their MIME type (`image/png`, `image/jpeg`).
    Bytes { data: Vec<u8>, mime: String },
}

/// One trace.moe hit (worker a).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageMatchVm {
    pub media: MediaVm,
    pub episode: Option<u32>,
    /// Scene start / end in the episode.
    pub from_secs: f64,
    pub to_secs: f64,
    /// 0.0..=1.0
    pub similarity: f32,
    pub preview_image: Option<String>,
    pub preview_video: Option<String>,
}

/// A person (voice actor, staff) (workers a and c). `id` is the AniList staff id as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonVm {
    pub id: String,
    pub name: String,
    pub image: Option<String>,
    /// Voice actor language ("Japanese") or staff role ("Director"), display-ready.
    pub role: Option<String>,
}

/// Character page (worker a; classic `/character/:name`). `id` is the AniList character id as text.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterVm {
    pub id: String,
    pub name: String,
    pub native_name: Option<String>,
    pub alternative_names: Vec<String>,
    pub image: Option<String>,
    /// Markdown-ish description from AniList, cleaned like synopses.
    pub description: Option<String>,
    pub gender: Option<String>,
    pub age: Option<String>,
    /// Display-ready ("24 June").
    pub birthday: Option<String>,
    pub blood_type: Option<String>,
    pub favourites: Option<u32>,
    pub appearances: Vec<CharacterAppearanceVm>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharacterAppearanceVm {
    pub media: MediaVm,
    /// `MAIN | SUPPORTING | BACKGROUND` (i18n `character.role.<lowercase>`).
    pub role: String,
    pub voice_actors: Vec<PersonVm>,
}

/// Technical facts of a title (worker c; classic "facts" block of `AnimeDetailLayout`). Display-ready strings;
/// `None` / empty when the provider does not say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaFactsVm {
    /// `TV`, `Movie`, ... (i18n `details.format.<lowercase>` when known).
    pub format: Option<String>,
    /// Original material (`Manga`, `Light novel`, ...).
    pub source: Option<String>,
    /// `winter` .. `fall` (lowercase) + `season_year`.
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub duration_min: Option<u32>,
    /// ISO dates `YYYY-MM-DD` (partial dates trimmed: `2024-10`).
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub studios: Vec<String>,
    pub producers: Vec<String>,
    pub country: Option<String>,
    pub mean_score: Option<u32>,
    pub popularity: Option<u32>,
    pub favourites: Option<u32>,
    pub synonyms: Vec<String>,
    /// `(tag name, rank 0..=100)`, spoiler tags excluded.
    pub tags: Vec<(String, u8)>,
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
    pub kitsu_id: Option<u32>,
    pub adult: bool,
    /// Wave B (b): catalogues that contributed to these facts besides AniList (`mal`, `shikimori`, `tenrai`;
    /// `data-facts-source`).
    pub facts_sources: Vec<String>,
    /// MAL score rank (`#n`) and member count, when MAL / Shikimori know the title.
    pub rank: Option<u32>,
    pub members: Option<u32>,
    /// Absolute screenshot URLs (Shikimori).
    pub screenshots: Vec<String>,
}

/// A related title (worker c). `relation` is AniList's `PREQUEL | SEQUEL | SIDE_STORY | PARENT | ADAPTATION |
/// ALTERNATIVE | SPIN_OFF | CHARACTER | SUMMARY | OTHER | SOURCE | COMPILATION | CONTAINS` (i18n
/// `details.relation.<lowercase>`); `kind` is `anime` or `manga` (manga opens nothing until wave B).
#[derive(Debug, Clone, PartialEq)]
pub struct RelatedVm {
    pub media: MediaVm,
    pub relation: String,
    pub kind: String,
}

/// A character of a title with its voice actors (worker c).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRoleVm {
    /// AniList character id as text (opens `Route::CharacterPage`).
    pub id: String,
    pub name: String,
    pub image: Option<String>,
    /// `MAIN | SUPPORTING | BACKGROUND`.
    pub role: String,
    pub voice_actors: Vec<PersonVm>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrailerVm {
    /// `youtube` | `dailymotion`
    pub site: String,
    pub id: String,
    pub thumbnail: Option<String>,
}

impl TrailerVm {
    /// Watch URL for `Backend::open_url`.
    pub fn url(&self) -> String {
        match self.site.as_str() {
            "dailymotion" => format!("https://www.dailymotion.com/video/{}", self.id),
            _ => format!("https://www.youtube.com/watch?v={}", self.id),
        }
    }
}

/// Next episode to air (worker c; classic `NextEpisodeSchedule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiringVm {
    pub episode: u32,
    pub at_unix: i64,
}

/// Everything the details page shows beyond `DetailsVm` (worker c), loaded separately so the header never waits.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DetailsExtraVm {
    pub facts: MediaFactsVm,
    pub relations: Vec<RelatedVm>,
    pub characters: Vec<CharacterRoleVm>,
    pub staff: Vec<PersonVm>,
    pub recommendations: Vec<MediaVm>,
    pub trailer: Option<TrailerVm>,
    pub next_airing: Option<AiringVm>,
    /// `(site name, url)` external / streaming links.
    pub external_links: Vec<(String, String)>,
}

/// One step of the watch order / season chain (worker c; classic `AnimeSeasons`), prequels first.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchOrderEntryVm {
    pub media: MediaVm,
    /// 1-based position in the chain.
    pub position: u32,
    /// Relation to the previous entry (`SEQUEL`, `SIDE_STORY`, ...), empty for the first.
    pub relation: String,
    /// The title whose page is open.
    pub current: bool,
}

/// One subtitle cue (worker d), milliseconds. Text keeps the track's inline tags (`<i>`, ASS overrides) as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CueVm {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

/// `Backend::translate_cues` input (worker d). The provider and key come from `Settings::subs.translation`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslateRequestVm {
    pub cues: Vec<CueVm>,
    /// ISO 639-1; `None` = let the provider detect.
    pub source_lang: Option<String>,
    pub target_lang: String,
    /// Cache key (media id + episode + track) so a re-watch does not pay again; empty = no cache.
    pub cache_key: String,
}

/// `Backend::translation_test` result (worker d).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranslationTestVm {
    pub sample_in: String,
    pub sample_out: String,
    pub latency_ms: u32,
}

/// Discord Rich Presence state (worker e; classic browsing / viewing / watching / paused).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceVm {
    /// Nothing shown (also sent when the setting turns off).
    Clear,
    Browsing,
    Viewing { title: String },
    Watching { title: String, episode: u32, position_secs: u32, duration_secs: Option<u32>, paused: bool, poster: Option<String> },
}

/// Disk use of the data directory (worker f; Settings > Data).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StorageUsageVm {
    pub data_dir: String,
    /// Metadata / image / translation caches (what `clear_cache` removes).
    pub cache_bytes: u64,
    pub torrents_bytes: u64,
    pub logs_bytes: u64,
    pub extensions_bytes: u64,
    /// Free space on the volume, when known.
    pub free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormatVm {
    Json,
    Csv,
}

impl ExportFormatVm {
    pub const ALL: [ExportFormatVm; 2] = [Self::Json, Self::Csv];

    pub fn id(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Csv => "csv",
        }
    }
}

/// `Backend::check_updates` result (worker f; GitHub releases of this repository).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheckVm {
    /// This build (`CARGO_PKG_VERSION`).
    pub current: String,
    /// Latest release tag (`v0.2.0`), `None` when there is no release yet.
    pub latest: Option<String>,
    pub newer: bool,
    /// Release page for `Backend::open_url`.
    pub url: Option<String>,
    pub notes: Option<String>,
    pub checked_unix: i64,
}

/// Which OS features this build can offer (worker f; the Settings > System rows hide what is `false`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemCapsVm {
    pub platform: PlatformVm,
    pub autostart: bool,
    pub tray: bool,
    /// The app can register itself as the `magnet:` handler without an installer (worker i, `tk-app/src/magnet.rs`):
    /// Linux (`.desktop` + `xdg-mime`) and Windows (HKCU `Software\Classes\magnet`); false on macOS (needs the
    /// bundle's Info.plist) and in dev builds without a stable executable path.
    pub magnet_handler: bool,
}

/// List statuses, shared by AniList and MAL (worker g). `id()` is the i18n suffix `lists.status.<id>` and the
/// Library tab id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListStatusVm {
    Watching,
    Planning,
    Completed,
    Paused,
    Dropped,
    Repeating,
}

impl ListStatusVm {
    pub const ALL: [ListStatusVm; 6] = [Self::Watching, Self::Planning, Self::Completed, Self::Paused, Self::Dropped, Self::Repeating];

    pub fn id(self) -> &'static str {
        match self {
            Self::Watching => "watching",
            Self::Planning => "planning",
            Self::Completed => "completed",
            Self::Paused => "paused",
            Self::Dropped => "dropped",
            Self::Repeating => "repeating",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// A connected (or not) list service (worker g; Settings > Accounts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListAccountVm {
    pub service: ListServiceVm,
    pub connected: bool,
    pub username: Option<String>,
    pub avatar: Option<String>,
    pub profile_url: Option<String>,
    /// Queued offline changes waiting to be pushed.
    pub pending_changes: u32,
    pub last_sync_unix: Option<i64>,
}

/// One list entry as the UI shows it (worker g). `media.id` is the usual UI media id.
#[derive(Debug, Clone, PartialEq)]
pub struct ListEntryVm {
    pub media: MediaVm,
    pub service: ListServiceVm,
    pub status: ListStatusVm,
    /// Episodes watched.
    pub progress: u32,
    pub episodes_total: Option<u32>,
    /// 0..=100 on the service's own scale mapped to percent; `None` = unscored.
    pub score: Option<u8>,
    /// ISO dates.
    pub started: Option<String>,
    pub finished: Option<String>,
    pub repeats: u32,
    pub notes: Option<String>,
    pub updated_unix: i64,
    /// The service's entry id, when known.
    pub remote_id: Option<u64>,
    /// Written locally, not yet pushed (offline queue).
    pub pending: bool,
}

/// A partial update of a list entry (worker g); `None` = leave as is. `score: Some(0)` clears the score.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListEntryUpdateVm {
    pub status: Option<ListStatusVm>,
    pub progress: Option<u32>,
    pub score: Option<u8>,
    pub notes: Option<String>,
}

/// `Backend::list_sync_now` summary (worker g).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListSyncVm {
    pub pushed: u32,
    pub pulled: u32,
    pub conflicts: u32,
    /// Per-service errors, display-ready.
    pub errors: Vec<String>,
}

/// A local-vs-remote disagreement about one entry (worker g).
#[derive(Debug, Clone, PartialEq)]
pub struct ListConflictVm {
    pub id: i64,
    pub local: ListEntryVm,
    pub remote: ListEntryVm,
    pub detected_unix: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSideVm {
    Local,
    Remote,
}

/// `Backend::import_list` summary (worker g; MAL XML export or AniList JSON).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ListImportVm {
    pub imported: u32,
    pub skipped: u32,
    pub errors: Vec<String>,
}

/// One row of the torrent session history (worker i; classic "Session history"): every torrent the engine ran,
/// newest first. Mirrors `tk_core::store::torrents::TorrentSessionRow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentSessionVm {
    pub id: i64,
    pub torrent_id: String,
    pub info_hash: String,
    pub name: String,
    pub media_id: Option<String>,
    pub series: Option<String>,
    pub episode: Option<u32>,
    pub started_unix: i64,
    /// `None` = still in the engine.
    pub ended_unix: Option<i64>,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub completed: bool,
}

/// What `Backend::torrent_preview` returns for a magnet / `.torrent` added by hand (worker i; classic
/// `MagnetAlignmentModal`): its files and a suggested `(file index, episode)` alignment with a confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct TorrentPreviewVm {
    pub name: String,
    pub info_hash: String,
    pub size_bytes: u64,
    pub files: Vec<TorrentFileVm>,
    /// `tk_core::release::parse_release` per video file; files without a number are left out.
    pub suggested: Vec<(usize, u32)>,
    /// 0.0..=1.0 share of video files that got an episode number.
    pub confidence: f32,
}

/// Debrid services (worker i). `id()` is the i18n suffix `debrid.service.<id>` and the settings key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebridServiceVm {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl DebridServiceVm {
    pub const ALL: [DebridServiceVm; 4] = [Self::RealDebrid, Self::TorBox, Self::AllDebrid, Self::Premiumize];

    pub fn id(self) -> &'static str {
        match self {
            Self::RealDebrid => "real_debrid",
            Self::TorBox => "torbox",
            Self::AllDebrid => "alldebrid",
            Self::Premiumize => "premiumize",
        }
    }

    /// Brand name (never translated).
    pub fn label(self) -> &'static str {
        match self {
            Self::RealDebrid => "Real-Debrid",
            Self::TorBox => "TorBox",
            Self::AllDebrid => "AllDebrid",
            Self::Premiumize => "Premiumize",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// A debrid account (worker i; Torrents > Debrid). The key itself never leaves the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebridAccountVm {
    pub service: DebridServiceVm,
    pub connected: bool,
    pub username: Option<String>,
    pub premium_until_unix: Option<i64>,
    /// Why the key was rejected, display-ready.
    pub error: Option<String>,
}

/// Cache status of a torrent on a debrid service (`SourceVm::debrid`, worker i).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebridStatusVm {
    pub service: DebridServiceVm,
    pub cached: bool,
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
    /// Local-first history write (exact position; paired phones pull it over Tatakai Link `GET /v1/history`).
    async fn record_progress(&self, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) -> UiResult<()>;

    // --- extensions ---
    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>>;
    async fn install_extension(&self, path: &str) -> UiResult<ExtensionVm>;
    async fn set_extension_enabled(&self, id: &str, enabled: bool) -> UiResult<()>;
    async fn remove_extension(&self, id: &str) -> UiResult<()>;

    // --- settings --- (no own account: identity is AniList / MAL, see `list_connect`; plan 14 section 0.1 #9)
    async fn load_settings(&self) -> UiResult<Settings>;
    async fn save_settings(&self, settings: &Settings) -> UiResult<()>;

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
    /// Downloads the Anime4K / FSR shader files the presets need into the player's shader folder, each verified
    /// against the SHA-256 of `packaging/{anime4k,fsr}.lock.json` (files already present and valid are kept).
    /// Returns how many files were downloaded. Called by the player the first time a preset finds them missing.
    async fn fetch_shaders(&self) -> UiResult<u32> {
        Err(UiError::not_wired("fetch_shaders"))
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

    // --- wave A (docs/redesign/15-parity-wave-a-contract.md section 3). Same rule: defaults return `NotWired`
    // so every implementation keeps compiling; `LiveBackend` delegates each one to `live/<area>.rs`. ---

    // ----- a: search, feeds, character pages -----

    /// Filtered, paginated search (`SearchQueryVm`), through the metadata router with AniList's real filters.
    async fn search_filtered(&self, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("search_filtered"))
    }
    /// Search inside one installed extension (`search` route of `.kai` bundles); results carry `MediaVm::id` when
    /// the extension maps to AniList / MAL, else an `ext:<id>:<key>` id the details page can resolve.
    async fn search_in_extension(&self, _ext_id: &str, _query: &str, _page: u32) -> UiResult<Vec<MediaVm>> {
        Err(UiError::not_wired("search_in_extension"))
    }
    /// One page of a home feed ("See all"): `feed` is the suffix of `SectionVm::title_key` (`trending`, `airing`,
    /// `seasonal`, `popular`, `top_rated`, `upcoming`, `recently_completed`).
    async fn feed(&self, _feed: &str, _page: u32) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("feed"))
    }
    /// Titles by studio / producer name (classic `/search/producer/:name`).
    async fn search_by_studio(&self, _studio: &str, _page: u32) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("search_by_studio"))
    }
    /// Scene search through trace.moe (`tk_core::tracemoe`), best match first.
    async fn search_by_image(&self, _image: ImageQueryVm) -> UiResult<Vec<ImageMatchVm>> {
        Err(UiError::not_wired("search_by_image"))
    }
    /// Character page (AniList character id as text).
    async fn character(&self, _character_id: &str) -> UiResult<CharacterVm> {
        Err(UiError::not_wired("character"))
    }

    // ----- c: details -----

    /// Relations, characters, staff, recommendations, trailer, next airing episode and the technical facts.
    async fn details_extra(&self, _id: &str) -> UiResult<DetailsExtraVm> {
        Err(UiError::not_wired("details_extra"))
    }
    /// Season chain / watch order of a title (prequels first), the open title marked `current`.
    async fn watch_order(&self, _id: &str) -> UiResult<Vec<WatchOrderEntryVm>> {
        Err(UiError::not_wired("watch_order"))
    }

    // ----- d: subtitles, translation, capture -----

    /// Download an external subtitle track (SRT / VTT / ASS text) with the stream's headers, for translation and
    /// the cue overlay. The player itself loads tracks through mpv (`Player::add_subtitle`).
    async fn fetch_subtitle(&self, _url: &str, _headers: &[(String, String)]) -> UiResult<String> {
        Err(UiError::not_wired("fetch_subtitle"))
    }
    /// Store (`Some`) or remove (`None`) the API key of a translation provider in `tk_core::Secrets`
    /// (`keys::translation(provider.id())`); never in `ui-settings.json`. `TranslationProviderVm::needs_key()` says
    /// which providers take one.
    async fn set_translation_key(&self, _provider: crate::model::TranslationProviderVm, _key: Option<&str>) -> UiResult<()> {
        Err(UiError::not_wired("set_translation_key"))
    }
    /// Whether a key is stored for `provider` (the UI shows "Key saved" and a Remove button, never the key).
    async fn translation_key_present(&self, _provider: crate::model::TranslationProviderVm) -> UiResult<bool> {
        Err(UiError::not_wired("translation_key_present"))
    }
    /// Translate cues with the configured provider (`Settings::subs.translation`), cached in the store by
    /// `TranslateRequestVm::cache_key`. Returns the same cues (same `index`) with translated `text`.
    async fn translate_cues(&self, _req: &TranslateRequestVm) -> UiResult<Vec<CueVm>> {
        Err(UiError::not_wired("translate_cues"))
    }
    /// Translate one sample sentence with the saved provider settings (Settings > Subtitles > "Test").
    async fn translation_test(&self) -> UiResult<TranslationTestVm> {
        Err(UiError::not_wired("translation_test"))
    }
    /// Where the next screenshot goes (`<Pictures>/Tatakai/<title> - E<episode> - <timestamp>.png`, parent created).
    /// The UI then calls `Player::screenshot_to_file(path)`.
    async fn screenshot_path(&self, _title: &str, _episode: u32) -> UiResult<PathBuf> {
        Err(UiError::not_wired("screenshot_path"))
    }

    // ----- e: Discord Rich Presence -----

    /// Update the Discord presence (no-op when `Settings::flow.discord_presence` is off or Discord is not running:
    /// never an error for the UI).
    async fn presence(&self, _state: &PresenceVm) -> UiResult<()> {
        Err(UiError::not_wired("presence"))
    }

    // ----- f: system, first-run wizard, reset -----

    /// "Reset app" (Settings > System, danger zone; classic `system:reset app data`): stop Link / torrents, delete
    /// `ui-settings.json`, `data.db`, `secrets.json` + the keyring entries, `config.json`, caches, extensions and
    /// the web view profile; with `keep_downloads` the torrent download folder survives. The window then shows the
    /// first-run wizard again (`Settings::system.setup_done` is false after the reset). Never deletes outside the
    /// data directory and the download folder.
    async fn reset_app(&self, _keep_downloads: bool) -> UiResult<()> {
        Err(UiError::not_wired("reset_app"))
    }
    /// Sizes of the data directory (Settings > Data).
    async fn storage_usage(&self) -> UiResult<StorageUsageVm> {
        Err(UiError::not_wired("storage_usage"))
    }
    /// Metadata cache rows and bytes `(rows, bytes)` (Settings > Data, metadata cache row).
    async fn metadata_cache_usage(&self) -> UiResult<(u64, u64)> {
        Err(UiError::not_wired("metadata_cache_usage"))
    }
    /// Empty the metadata cache; returns the rows removed.
    async fn clear_metadata_cache(&self) -> UiResult<u64> {
        Err(UiError::not_wired("clear_metadata_cache"))
    }
    /// "Clear watch history" (Settings > Data, after a confirmation); returns the entries removed.
    async fn clear_watch_history(&self) -> UiResult<u32> {
        Err(UiError::not_wired("clear_watch_history"))
    }
    /// Unix time of the last history sync with a phone over Tatakai Link (`None` = never).
    async fn history_last_sync_unix(&self) -> UiResult<Option<i64>> {
        Err(UiError::not_wired("history_last_sync_unix"))
    }
    /// Remove the caches (metadata cache rows, images, translations, web view cache); returns the bytes freed.
    async fn clear_cache(&self) -> UiResult<u64> {
        Err(UiError::not_wired("clear_cache"))
    }
    /// Open the data directory in the file manager.
    async fn open_data_folder(&self) -> UiResult<()> {
        Err(UiError::not_wired("open_data_folder"))
    }
    /// Open the logs directory (or the current log file) with the system handler.
    async fn open_logs(&self) -> UiResult<()> {
        Err(UiError::not_wired("open_logs"))
    }
    /// Write the local history to `<Downloads>/tatakai-history-<date>.<ext>`; returns the file.
    async fn export_history(&self, _format: ExportFormatVm) -> UiResult<PathBuf> {
        Err(UiError::not_wired("export_history"))
    }
    /// `THIRD_PARTY_NOTICES.md` of this build (Markdown text) for the Licenses sheet.
    async fn licenses_text(&self) -> UiResult<String> {
        Err(UiError::not_wired("licenses_text"))
    }
    /// Ask GitHub releases of this repository for a newer version (never auto-downloads).
    async fn check_updates(&self) -> UiResult<UpdateCheckVm> {
        Err(UiError::not_wired("check_updates"))
    }
    /// What the OS integration offers on this platform / build.
    fn system_caps(&self) -> SystemCapsVm {
        SystemCapsVm { platform: PlatformVm::current(), autostart: false, tray: false, magnet_handler: false }
    }
    /// Whether the OS autostart entry exists right now (may differ from `Settings::system.start_with_system`).
    async fn autostart_enabled(&self) -> UiResult<bool> {
        Err(UiError::not_wired("autostart_enabled"))
    }
    /// Create / remove the OS autostart entry (`.desktop` in `~/.config/autostart`, `Run` registry key).
    async fn set_autostart(&self, _enabled: bool) -> UiResult<()> {
        Err(UiError::not_wired("set_autostart"))
    }
    /// Open an `http(s)` URL in the system browser (trailers, release pages, OAuth fallbacks, profiles).
    async fn open_url(&self, _url: &str) -> UiResult<()> {
        Err(UiError::not_wired("open_url"))
    }

    // ----- g: lists (AniList / MAL) -----

    /// Both services, connected or not (Settings > Accounts, Library header).
    async fn list_accounts(&self) -> UiResult<Vec<ListAccountVm>> {
        Err(UiError::not_wired("list_accounts"))
    }
    /// OAuth (PKCE for MAL, implicit/code for AniList) through the system browser and a loopback redirect on
    /// `127.0.0.1`; resolves when the service answers or after a timeout (`ErrorKind::Auth`). Cancel-safe.
    async fn list_connect(&self, _service: ListServiceVm) -> UiResult<ListAccountVm> {
        Err(UiError::not_wired("list_connect"))
    }
    async fn list_disconnect(&self, _service: ListServiceVm) -> UiResult<()> {
        Err(UiError::not_wired("list_disconnect"))
    }
    /// The entry of a title on the primary (or only) connected service; `None` when not in any list.
    async fn list_entry(&self, _media_id: &str) -> UiResult<Option<ListEntryVm>> {
        Err(UiError::not_wired("list_entry"))
    }
    /// Create or update the entry (local write + offline queue + push when online). Returns the stored entry.
    async fn set_list_entry(&self, _media_id: &str, _update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
        Err(UiError::not_wired("set_list_entry"))
    }
    async fn remove_list_entry(&self, _media_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("remove_list_entry"))
    }
    /// Entries of the local mirror, newest update first; `status: None` = every status.
    async fn list_entries(&self, _status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
        Err(UiError::not_wired("list_entries"))
    }
    async fn favorites(&self) -> UiResult<Vec<MediaVm>> {
        Err(UiError::not_wired("favorites"))
    }
    async fn is_favorite(&self, _media_id: &str) -> UiResult<bool> {
        Err(UiError::not_wired("is_favorite"))
    }
    async fn set_favorite(&self, _media_id: &str, _favorite: bool) -> UiResult<()> {
        Err(UiError::not_wired("set_favorite"))
    }
    /// Push the offline queue and pull every connected service; disagreements become `list_conflicts`.
    async fn list_sync_now(&self) -> UiResult<ListSyncVm> {
        Err(UiError::not_wired("list_sync_now"))
    }
    async fn list_conflicts(&self) -> UiResult<Vec<ListConflictVm>> {
        Err(UiError::not_wired("list_conflicts"))
    }
    async fn resolve_list_conflict(&self, _conflict_id: i64, _keep: ConflictSideVm) -> UiResult<()> {
        Err(UiError::not_wired("resolve_list_conflict"))
    }
    /// Import a MAL XML export or an AniList JSON export from a local file into the local mirror (+ queue).
    async fn import_list(&self, _path: &str) -> UiResult<ListImportVm> {
        Err(UiError::not_wired("import_list"))
    }
    /// Write the local mirror to `<Downloads>/tatakai-lists-<date>.<ext>`; returns the file.
    async fn export_list(&self, _format: ExportFormatVm) -> UiResult<PathBuf> {
        Err(UiError::not_wired("export_list"))
    }

    // ----- i: manual torrents, debrid -----

    /// Inspect a magnet link, a `.torrent` file path or a `.torrent` URL without downloading its data: files and a
    /// suggested episode alignment.
    async fn torrent_preview(&self, _source: &str) -> UiResult<TorrentPreviewVm> {
        Err(UiError::not_wired("torrent_preview"))
    }
    /// Add the torrent to the library with the confirmed `(file index, episode)` alignment, labelled for `media_id`
    /// (`None` = unlabelled). Returns the torrent id.
    async fn torrent_add_manual(&self, _source: &str, _media_id: Option<&str>, _alignment: &[(usize, u32)]) -> UiResult<String> {
        Err(UiError::not_wired("torrent_add_manual"))
    }
    /// Start fetching the next episode's file (same batch) or its best torrent in the background while `episode`
    /// plays (`TorrentSettingsVm::preload_next`).
    async fn torrent_preload_next(&self, _id: &str, _episode: u32) -> UiResult<()> {
        Err(UiError::not_wired("torrent_preload_next"))
    }
    /// A `magnet:` link the OS handed to the app (command line at launch, or forwarded by a second instance) that
    /// the torrent client has not consumed yet; `None` otherwise. The Torrents > Library page polls it on mount and
    /// opens the "Add torrent" sheet with it (`tk-app/src/magnet.rs`).
    async fn take_pending_magnet(&self) -> UiResult<Option<String>> {
        Err(UiError::not_wired("take_pending_magnet"))
    }
    /// Whether this app is the OS handler of the `magnet:` scheme right now (`SystemCapsVm::magnet_handler` says
    /// if that can be changed from here).
    async fn magnet_handler_registered(&self) -> UiResult<bool> {
        Err(UiError::not_wired("magnet_handler_registered"))
    }
    /// Register / unregister the app as the `magnet:` handler (per-user, no installer, no elevation).
    async fn set_magnet_handler(&self, _enabled: bool) -> UiResult<()> {
        Err(UiError::not_wired("set_magnet_handler"))
    }
    /// Torrent session history, newest first (`torrent_sessions` table; Torrents > History).
    async fn torrent_session_history(&self, _limit: u32) -> UiResult<Vec<TorrentSessionVm>> {
        Err(UiError::not_wired("torrent_session_history"))
    }
    /// Delete the closed rows of the session history; returns how many were removed.
    async fn torrent_clear_session_history(&self) -> UiResult<u32> {
        Err(UiError::not_wired("torrent_clear_session_history"))
    }
    /// Force an announce to every tracker of a torrent (Torrents > Trackers "Reannounce"); the status column then
    /// reflects the real answer (`TrackerStatusVm::Working` / `Failed` with its message), not `Unknown`.
    async fn torrent_reannounce(&self, _torrent_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("torrent_reannounce"))
    }
    async fn debrid_accounts(&self) -> UiResult<Vec<DebridAccountVm>> {
        Err(UiError::not_wired("debrid_accounts"))
    }
    /// Save (`Some`) or remove (`None`) a service API key, validating it against the service.
    async fn set_debrid_key(&self, _service: DebridServiceVm, _key: Option<&str>) -> UiResult<DebridAccountVm> {
        Err(UiError::not_wired("set_debrid_key"))
    }
    /// Save the debrid preference order (most preferred first; partial lists keep the other services after it).
    /// Returns the saved order.
    async fn set_debrid_order(&self, _order: &[DebridServiceVm]) -> UiResult<Vec<DebridServiceVm>> {
        Err(UiError::not_wired("set_debrid_order"))
    }
    /// Play a torrent source through a connected debrid service (cached only): resolves the episode file to a
    /// direct HTTPS link, returned like a web stream.
    async fn debrid_play(&self, _source: &str, _id: &str, _episode: u32) -> UiResult<PlaybackVm> {
        Err(UiError::not_wired("debrid_play"))
    }

    // --- wave B (docs/redesign/16-parity-wave-b-contract.md section 3). Same rule: defaults return `NotWired`;
    // `LiveBackend` delegates each one to `live/<area>.rs`; nobody edits the signatures. ---

    // ----- b: home, discover, calendar, recommendations, metadata providers (`live/home.rs`) -----

    /// Recently aired episodes, newest first (AniList `airingSchedules(notYetAired: false)`), paginated.
    async fn latest_episodes(&self, _page: u32) -> UiResult<Vec<LatestEpisodeVm>> {
        Err(UiError::not_wired("latest_episodes"))
    }
    /// Top 10 of the range (trending today / week / month), ranked.
    async fn top10(&self, _range: TopRangeVm) -> UiResult<Vec<MediaVm>> {
        Err(UiError::not_wired("top10"))
    }
    /// "Because you watched" rows: up to 3 seeds from the local history with AniList recommendations each.
    async fn because_you_watched(&self) -> UiResult<Vec<BecauseVm>> {
        Err(UiError::not_wired("because_you_watched"))
    }
    /// AniList `GenreCollection` (cached a day).
    async fn genres(&self) -> UiResult<Vec<String>> {
        Err(UiError::not_wired("genres"))
    }
    /// One page of titles of a genre (infinite genre rows; "See all" of a genre row reuses `Route::BrowsePage` with
    /// `feed = "genre:<name>"`, which worker a's `feed()` does not know: b routes it here).
    async fn genre_feed(&self, _genre: &str, _page: u32) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("genre_feed"))
    }
    /// Episodes aired since the last Home visit for titles in the Watching list (`home.followed-updates`).
    async fn followed_updates(&self) -> UiResult<Vec<LatestEpisodeVm>> {
        Err(UiError::not_wired("followed_updates"))
    }
    /// Airing schedule of the 7 days starting at `week_start_unix` (local midnight), `tracked` from `tracked_shows`.
    async fn airing_schedule(&self, _week_start_unix: i64) -> UiResult<Vec<AiringSlotVm>> {
        Err(UiError::not_wired("airing_schedule"))
    }
    /// "My shows" of the calendar (`tracked_shows` table; `details.track-show`, `home.calendar.my-shows`).
    async fn tracked_shows(&self) -> UiResult<Vec<MediaVm>> {
        Err(UiError::not_wired("tracked_shows"))
    }
    async fn is_tracked(&self, _media_id: &str) -> UiResult<bool> {
        Err(UiError::not_wired("is_tracked"))
    }
    async fn set_tracked(&self, _media_id: &str, _tracked: bool) -> UiResult<()> {
        Err(UiError::not_wired("set_tracked"))
    }
    /// Tracked episodes airing within `Settings::home.reminder_minutes` that were not announced yet (the shell shows
    /// a snackbar / tray notice; `home.calendar.reminders`). Marks them announced.
    async fn reminders_due(&self) -> UiResult<Vec<AiringSlotVm>> {
        Err(UiError::not_wired("reminders_due"))
    }
    /// "For you" page, scored against the local taste profile (`tk_core::recs`), with explanations.
    async fn recommendations(&self, _page: u32) -> UiResult<Vec<RecommendationVm>> {
        Err(UiError::not_wired("recommendations"))
    }
    /// Store feedback on a recommendation (`recs_feedback` table; local only).
    async fn recs_feedback(&self, _media_id: &str, _feedback: RecsFeedbackVm) -> UiResult<()> {
        Err(UiError::not_wired("recs_feedback"))
    }
    async fn taste_profile(&self) -> UiResult<TasteProfileVm> {
        Err(UiError::not_wired("taste_profile"))
    }
    /// Points of the Anime Atlas map (`home.recs.atlas`): the user's titles + recommendations, 2-D projected.
    async fn atlas_points(&self) -> UiResult<Vec<AtlasPointVm>> {
        Err(UiError::not_wired("atlas_points"))
    }
    /// Torrent engine summary for the Home banner (`home.torrent-activity`); `active == 0` hides it.
    async fn torrent_activity(&self) -> UiResult<TorrentActivityVm> {
        Err(UiError::not_wired("torrent_activity"))
    }
    /// Health of every metadata provider (Settings > Metadata; `settings.metadata.extra-providers`).
    async fn provider_health(&self) -> UiResult<Vec<ProviderHealthVm>> {
        Err(UiError::not_wired("provider_health"))
    }

    // ----- h: downloads engine, Downloads page, local library, auto-download (`live/downloads.rs`) -----

    /// Queue + history rows, active first then newest first (`downloads` table, torrents mirrored in).
    async fn downloads(&self) -> UiResult<Vec<DownloadVm>> {
        Err(UiError::not_wired("downloads"))
    }
    /// Queue one episode (HLS or MP4 picked from the ranked sources). Returns the row id.
    async fn download_enqueue(&self, _media_id: &str, _episode: u32, _req: &DownloadRequestVm) -> UiResult<i64> {
        Err(UiError::not_wired("download_enqueue"))
    }
    /// Queue several episodes of a title ("Download season" sheet). Returns the rows created.
    async fn download_enqueue_many(&self, _media_id: &str, _episodes: &[u32], _req: &DownloadRequestVm) -> UiResult<Vec<i64>> {
        Err(UiError::not_wired("download_enqueue_many"))
    }
    async fn download_pause(&self, _id: i64) -> UiResult<()> {
        Err(UiError::not_wired("download_pause"))
    }
    /// Resume a paused / failed row from its last completed segment (`downloads.resume`).
    async fn download_resume(&self, _id: i64) -> UiResult<()> {
        Err(UiError::not_wired("download_resume"))
    }
    async fn download_cancel(&self, _id: i64, _delete_file: bool) -> UiResult<()> {
        Err(UiError::not_wired("download_cancel"))
    }
    /// Remove a finished / cancelled row from the history (`delete_file` also removes the data).
    async fn download_remove(&self, _id: i64, _delete_file: bool) -> UiResult<()> {
        Err(UiError::not_wired("download_remove"))
    }
    /// Drop every non-active row; returns how many went.
    async fn download_clear_history(&self) -> UiResult<u32> {
        Err(UiError::not_wired("download_clear_history"))
    }
    async fn download_open_folder(&self, _id: i64) -> UiResult<()> {
        Err(UiError::not_wired("download_open_folder"))
    }
    async fn download_storage(&self) -> UiResult<DownloadStorageVm> {
        Err(UiError::not_wired("download_storage"))
    }
    /// A completed download or local-library file of `episode`, as a playable local stream (`file://` or the
    /// loopback URL the engine serves); `None` when nothing is on disk. The player asks before resolving online.
    async fn local_playback(&self, _media_id: &str, _episode: u32) -> UiResult<Option<PlaybackVm>> {
        Err(UiError::not_wired("local_playback"))
    }
    async fn local_library(&self) -> UiResult<Vec<LocalItemVm>> {
        Err(UiError::not_wired("local_library"))
    }
    /// Scan `Settings::downloads.library_dirs` (or `dir` when given) for video files; new files are matched by their
    /// release name (`tk_core::release`) and the title search.
    async fn library_scan(&self, _dir: Option<&str>) -> UiResult<LibraryScanVm> {
        Err(UiError::not_wired("library_scan"))
    }
    /// Import files picked by the user (absolute paths) into the local library.
    async fn library_import(&self, _paths: &[String]) -> UiResult<LibraryScanVm> {
        Err(UiError::not_wired("library_import"))
    }
    /// Set / fix the title + episode of a local file (`downloads.local-library.match`).
    async fn local_match(&self, _item_id: i64, _media_id: &str, _episode: u32) -> UiResult<()> {
        Err(UiError::not_wired("local_match"))
    }
    async fn local_remove(&self, _item_id: i64, _delete_file: bool) -> UiResult<()> {
        Err(UiError::not_wired("local_remove"))
    }
    /// Run the auto-downloader now (`downloads.auto.new-episodes`); also runs on the schedule when enabled.
    async fn auto_download_check_now(&self) -> UiResult<AutoDownloadRunVm> {
        Err(UiError::not_wired("auto_download_check_now"))
    }
    /// The last pass, if any.
    async fn auto_download_last_run(&self) -> UiResult<Option<AutoDownloadRunVm>> {
        Err(UiError::not_wired("auto_download_last_run"))
    }
    /// Re-check a completed row on disk (`downloads.integrity-check`): missing / truncated -> `Broken`. The engine
    /// also runs this over every completed row at start-up.
    async fn download_verify(&self, _id: i64) -> UiResult<DownloadVm> {
        Err(UiError::not_wired("download_verify"))
    }

    // ----- j: extension servers, Stremio, updates, preferences, repositories, custom providers
    // (`live/ext_server.rs`, `live/ext_catalog.rs`) -----

    async fn ext_servers(&self) -> UiResult<Vec<ExtServerVm>> {
        Err(UiError::not_wired("ext_servers"))
    }
    /// Pair with a PC's extension server (`POST /api/server/v1/auth/pair` with a code, `/auth/login` with a
    /// password, or `/health` only when anonymous viewers are allowed). The token goes to `Secrets`.
    async fn ext_server_pair(&self, _req: &ExtServerPairVm) -> UiResult<ExtServerVm> {
        Err(UiError::not_wired("ext_server_pair"))
    }
    async fn ext_server_set_enabled(&self, _id: &str, _enabled: bool) -> UiResult<()> {
        Err(UiError::not_wired("ext_server_set_enabled"))
    }
    /// Remove the server and its token (also `POST /auth/logout` when reachable).
    async fn ext_server_remove(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("ext_server_remove"))
    }
    /// `GET /health` + `GET /sources`: updates `online`, `sources`, `last_error`.
    async fn ext_server_test(&self, _id: &str) -> UiResult<ExtServerVm> {
        Err(UiError::not_wired("ext_server_test"))
    }
    async fn ext_server_sources(&self, _id: &str) -> UiResult<Vec<ExtSourceVm>> {
        Err(UiError::not_wired("ext_server_sources"))
    }
    /// A `tatakai://extensionserver/connect?server=...&code=...` link handed to the app (command line or second
    /// instance), not consumed yet; the Extensions page opens the pairing sheet with it after confirmation.
    async fn take_pending_server_link(&self) -> UiResult<Option<String>> {
        Err(UiError::not_wired("take_pending_server_link"))
    }
    async fn stremio_addons(&self) -> UiResult<Vec<StremioAddonVm>> {
        Err(UiError::not_wired("stremio_addons"))
    }
    /// Fetch `manifest.json` of a Stremio add-on and store it.
    async fn stremio_add(&self, _manifest_url: &str) -> UiResult<StremioAddonVm> {
        Err(UiError::not_wired("stremio_add"))
    }
    async fn stremio_remove(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("stremio_remove"))
    }
    async fn stremio_set_enabled(&self, _id: &str, _enabled: bool) -> UiResult<()> {
        Err(UiError::not_wired("stremio_set_enabled"))
    }
    /// Compare installed extensions with their origin (repository catalogue or install URL); cached a day.
    async fn ext_updates_check(&self) -> UiResult<Vec<ExtUpdateVm>> {
        Err(UiError::not_wired("ext_updates_check"))
    }
    /// Download and install the newer package of one extension, keeping its preferences and enabled state.
    async fn ext_update(&self, _id: &str) -> UiResult<ExtensionVm> {
        Err(UiError::not_wired("ext_update"))
    }
    /// Update everything with a known newer version; returns how many were updated.
    async fn ext_update_all(&self) -> UiResult<u32> {
        Err(UiError::not_wired("ext_update_all"))
    }
    /// "Disable all" / "Enable all" without uninstalling (`ext.kill-switch`); returns how many changed. Disabling
    /// remembers which ones were on (`settings` key `ext.kill_switch.previous`) so enabling restores exactly those.
    async fn set_all_extensions_enabled(&self, _enabled: bool) -> UiResult<u32> {
        Err(UiError::not_wired("set_all_extensions_enabled"))
    }
    /// Install a `.kai` (or a package the host understands) from an `https` URL (`ext.install-url`).
    async fn install_extension_url(&self, _url: &str) -> UiResult<ExtensionVm> {
        Err(UiError::not_wired("install_extension_url"))
    }
    /// Fields the extension declares + stored values (`ext_prefs` table; exposed to the bundle as `preferences`).
    async fn ext_preferences(&self, _id: &str) -> UiResult<Vec<ExtPrefFieldVm>> {
        Err(UiError::not_wired("ext_preferences"))
    }
    async fn set_ext_preference(&self, _id: &str, _key: &str, _value: &str) -> UiResult<()> {
        Err(UiError::not_wired("set_ext_preference"))
    }
    async fn ext_repos(&self) -> UiResult<Vec<ExtRepoVm>> {
        Err(UiError::not_wired("ext_repos"))
    }
    /// Add a repository by URL (JSON index of packages) and fetch its catalogue.
    async fn ext_repo_add(&self, _url: &str) -> UiResult<ExtRepoVm> {
        Err(UiError::not_wired("ext_repo_add"))
    }
    async fn ext_repo_remove(&self, _id: &str) -> UiResult<()> {
        Err(UiError::not_wired("ext_repo_remove"))
    }
    /// Catalogue of a repository (cached; `refresh` fetches again).
    async fn ext_repo_catalog(&self, _id: &str, _refresh: bool) -> UiResult<Vec<ExtCatalogItemVm>> {
        Err(UiError::not_wired("ext_repo_catalog"))
    }
    /// Installed extensions that declare custom providers (manifest `customSources`).
    async fn custom_providers(&self) -> UiResult<Vec<CustomProviderVm>> {
        Err(UiError::not_wired("custom_providers"))
    }
    /// Home rows of one provider (`custom/home` route): `SectionVm::title_key` = `ext:<name>:<row>` (display text
    /// after the prefix, like the existing `ext:` rows of Home).
    async fn provider_home(&self, _ext_id: &str) -> UiResult<Vec<SectionVm>> {
        Err(UiError::not_wired("provider_home"))
    }
    /// One page of a provider row ("See all" on a provider page).
    async fn provider_feed(&self, _ext_id: &str, _row: &str, _page: u32) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("provider_feed"))
    }
    /// A provider title (`custom/info`): `item_key` is the part after `ext:<ext_id>:` of the id.
    async fn provider_item(&self, _ext_id: &str, _item_key: &str) -> UiResult<ProviderItemVm> {
        Err(UiError::not_wired("provider_item"))
    }
    /// Bookmarked provider titles (`provider_bookmarks` table; `ext.custom-providers.bookmark`).
    async fn provider_bookmarks(&self) -> UiResult<Vec<MediaVm>> {
        Err(UiError::not_wired("provider_bookmarks"))
    }
    async fn set_provider_bookmark(&self, _ext_id: &str, _item_key: &str, _bookmarked: bool) -> UiResult<()> {
        Err(UiError::not_wired("set_provider_bookmark"))
    }

    // ----- k: native extension server, proxy, FlareSolverr, embed sniffer, Link extras (`live/server.rs`,
    // `live/sniffer.rs`, `live/link.rs`) -----

    async fn server_status(&self) -> UiResult<ServerStatusVm> {
        Err(UiError::not_wired("server_status"))
    }
    /// Start `tk-server` on `Settings::server.port` (a taken port is reported in `ServerStatusVm::error`).
    async fn server_start(&self) -> UiResult<ServerStatusVm> {
        Err(UiError::not_wired("server_start"))
    }
    async fn server_stop(&self) -> UiResult<()> {
        Err(UiError::not_wired("server_stop"))
    }
    /// One-time pairing code + deep link + QR for a phone (`server.pairing`).
    async fn server_open_pairing(&self) -> UiResult<ServerPairingVm> {
        Err(UiError::not_wired("server_open_pairing"))
    }
    async fn server_devices(&self) -> UiResult<Vec<ServerDeviceVm>> {
        Err(UiError::not_wired("server_devices"))
    }
    async fn server_revoke(&self, _device_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("server_revoke"))
    }
    /// Set (`Some`) or clear (`None`) the viewer password (argon2 hash in the server state; never in settings).
    async fn server_set_viewer_password(&self, _password: Option<&str>) -> UiResult<()> {
        Err(UiError::not_wired("server_set_viewer_password"))
    }
    /// Server log lines newer than `after` (`server.logs`).
    async fn server_logs(&self, _after: u64) -> UiResult<ServerLogPageVm> {
        Err(UiError::not_wired("server_logs"))
    }
    async fn server_proxy_status(&self) -> UiResult<ProxyStatusVm> {
        Err(UiError::not_wired("server_proxy_status"))
    }
    /// Empty the server's lookup + media caches.
    async fn server_clear_cache(&self) -> UiResult<()> {
        Err(UiError::not_wired("server_clear_cache"))
    }
    /// `GET <flaresolverr>/` health + a `request.get` round trip to `url` when given (`server.flaresolverr`).
    async fn flaresolverr_test(&self, _url: Option<&str>) -> UiResult<FlareSolverrVm> {
        Err(UiError::not_wired("flaresolverr_test"))
    }
    /// Open an embed page in a hidden web view and capture the media URL it requests (`sources.embed-sniffer`).
    /// `LiveBackend::playback_source` calls this for `SourceVm::embed` sources; the picker's "Sniff" action too.
    async fn sniff_embed(&self, _req: &SniffRequestVm) -> UiResult<SniffResultVm> {
        Err(UiError::not_wired("sniff_embed"))
    }
    /// Hand a title / episode / position to a paired phone over Tatakai Link (`link.send-to-device`).
    async fn send_to_device(&self, _req: &SendToDeviceVm) -> UiResult<()> {
        Err(UiError::not_wired("send_to_device"))
    }
    /// Long-poll: the next command a paired phone sent (`POST /v1/control`, `link.remote-control` /
    /// `link.handoff`). `app.rs::Shell` loops on it and stops looping on `ErrorKind::NotWired`.
    async fn wait_remote_command(&self) -> UiResult<RemoteCommandVm> {
        Err(UiError::not_wired("wait_remote_command"))
    }

    // ----- l: manga (`live/manga.rs`); the `read_*` calls are shared with novels (m consumes them) -----

    async fn manga_home(&self) -> UiResult<ReadHomeVm> {
        Err(UiError::not_wired("manga_home"))
    }
    /// Filtered manga search (`MediaKind::Manga` on AniList, MangaDex / MangaUpdates as fallbacks).
    async fn manga_search(&self, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("manga_search"))
    }
    async fn manga_details(&self, _id: &str) -> UiResult<MangaDetailsVm> {
        Err(UiError::not_wired("manga_details"))
    }
    /// Chapters per provider (installed extensions with the `manga` capability + enabled extension servers),
    /// cached in `chapter_lists`; `refresh` asks the providers again.
    async fn manga_chapters(&self, _id: &str, _refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
        Err(UiError::not_wired("manga_chapters"))
    }
    /// Pages of a chapter (`pages` route), offline copy first (`offline_chapters`).
    async fn manga_pages(&self, _chapter: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
        Err(UiError::not_wired("manga_pages"))
    }
    /// Reading position of a manga / novel title (`read_progress`), newest chapter first.
    async fn read_progress(&self, _kind: ReadKindVm, _media_id: &str) -> UiResult<Vec<ReadProgressVm>> {
        Err(UiError::not_wired("read_progress"))
    }
    /// Write a reading position (local; also advances the AniList manga list when `Settings::lists.auto_progress`).
    async fn set_read_progress(&self, _kind: ReadKindVm, _media_id: &str, _progress: &ReadProgressVm) -> UiResult<()> {
        Err(UiError::not_wired("set_read_progress"))
    }
    /// AniList manga list entry of a manga or novel (`MediaListCollection(type: MANGA)`); `None` = not listed.
    async fn read_list_entry(&self, _kind: ReadKindVm, _media_id: &str) -> UiResult<Option<ListEntryVm>> {
        Err(UiError::not_wired("read_list_entry"))
    }
    /// Create / update the manga list entry (`progress` = chapters read). Local mirror `read_list_entries` + queue.
    async fn set_read_list_entry(&self, _kind: ReadKindVm, _media_id: &str, _update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
        Err(UiError::not_wired("set_read_list_entry"))
    }
    async fn remove_read_list_entry(&self, _kind: ReadKindVm, _media_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("remove_read_list_entry"))
    }
    /// Entries of the local manga-list mirror of one kind, newest update first; `status: None` = every status.
    async fn read_list_entries(&self, _kind: ReadKindVm, _status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
        Err(UiError::not_wired("read_list_entries"))
    }
    /// Queue chapters for offline reading through the download engine (worker h's queue, kind `MangaChapter`).
    async fn manga_download_chapters(&self, _id: &str, _chapters: &[ChapterRefVm]) -> UiResult<u32> {
        Err(UiError::not_wired("manga_download_chapters"))
    }
    /// Remove the offline copy of a chapter.
    async fn read_remove_offline(&self, _kind: ReadKindVm, _chapter: &ChapterRefVm) -> UiResult<()> {
        Err(UiError::not_wired("read_remove_offline"))
    }
    /// Drop the cached chapter lists / pages of a title (`manga.chapters.tools` "Clear cache").
    async fn read_clear_cache(&self, _kind: ReadKindVm, _media_id: &str) -> UiResult<()> {
        Err(UiError::not_wired("read_clear_cache"))
    }

    // ----- m: novels + TTS (`live/novel.rs`, `tk-app/src/tts.rs`) -----

    async fn novel_home(&self) -> UiResult<ReadHomeVm> {
        Err(UiError::not_wired("novel_home"))
    }
    /// Novel search (AniList `MANGA` with format `NOVEL`, plus the novel extensions' `search`).
    async fn novel_search(&self, _q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        Err(UiError::not_wired("novel_search"))
    }
    async fn novel_details(&self, _id: &str) -> UiResult<NovelDetailsVm> {
        Err(UiError::not_wired("novel_details"))
    }
    /// Chapters per provider (`novelChapters`); `translator` filters by group when given.
    async fn novel_chapters(&self, _id: &str, _translator: Option<&str>, _refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
        Err(UiError::not_wired("novel_chapters"))
    }
    /// Text of a chapter (`novelContent`), offline copy first.
    async fn novel_content(&self, _chapter: &ChapterRefVm) -> UiResult<NovelContentVm> {
        Err(UiError::not_wired("novel_content"))
    }
    /// Ask every provider whether it has the title (`novel.details.check-sources`): `(ext name, chapters found)`.
    async fn novel_check_sources(&self, _id: &str) -> UiResult<Vec<(String, u32)>> {
        Err(UiError::not_wired("novel_check_sources"))
    }
    async fn novel_download_chapters(&self, _id: &str, _chapters: &[ChapterRefVm]) -> UiResult<u32> {
        Err(UiError::not_wired("novel_download_chapters"))
    }
    /// Voices of a provider (system voices enumerated from the OS; cloud voices from the API when a key exists).
    async fn tts_voices(&self, _provider: TtsProviderVm) -> UiResult<Vec<TtsVoiceVm>> {
        Err(UiError::not_wired("tts_voices"))
    }
    /// Synthesize `text` (cached in `tts_cache` by provider + voice + rate + text hash) or speak it natively.
    async fn tts_speak(&self, _req: &TtsRequestVm) -> UiResult<TtsSpeakVm> {
        Err(UiError::not_wired("tts_speak"))
    }
    /// Stop native speech (no-op for clips: the UI stops its `<audio>`).
    async fn tts_stop(&self) -> UiResult<()> {
        Err(UiError::not_wired("tts_stop"))
    }
    /// Store (`Some`) / remove (`None`) a provider API key in `Secrets` (`keys::tts(provider.id())`).
    async fn set_tts_key(&self, _provider: TtsProviderVm, _key: Option<&str>) -> UiResult<()> {
        Err(UiError::not_wired("set_tts_key"))
    }
    async fn tts_key_present(&self, _provider: TtsProviderVm) -> UiResult<bool> {
        Err(UiError::not_wired("tts_key_present"))
    }
    /// Local KittenTTS availability (`novel.tts.kitten`).
    async fn kitten_status(&self) -> UiResult<KittenStatusVm> {
        Err(UiError::not_wired("kitten_status"))
    }
    /// Download the KittenTTS model into `<data>/tts/kitten` (user-triggered; never automatic).
    async fn kitten_install(&self) -> UiResult<KittenStatusVm> {
        Err(UiError::not_wired("kitten_install"))
    }
    async fn kitten_remove(&self) -> UiResult<()> {
        Err(UiError::not_wired("kitten_remove"))
    }

    // ----- n: AniList social (`live/social.rs`) -----

    /// The viewer's profile (`None`) or another user's by name (`social.profile.page` / `public`).
    async fn anilist_profile(&self, _name: Option<&str>) -> UiResult<ProfileVm> {
        Err(UiError::not_wired("anilist_profile"))
    }
    async fn anilist_follow(&self, _user_id: u64, _follow: bool) -> UiResult<()> {
        Err(UiError::not_wired("anilist_follow"))
    }
    async fn anilist_followers(&self, _user_id: u64, _page: u32) -> UiResult<UserPageVm> {
        Err(UiError::not_wired("anilist_followers"))
    }
    async fn anilist_following(&self, _user_id: u64, _page: u32) -> UiResult<UserPageVm> {
        Err(UiError::not_wired("anilist_following"))
    }
    /// Activity feed page (`social.activity.feed`).
    async fn anilist_activity(&self, _feed: &ActivityFeedVm, _page: u32) -> UiResult<Vec<ActivityVm>> {
        Err(UiError::not_wired("anilist_activity"))
    }
    async fn anilist_activity_replies(&self, _activity_id: u64) -> UiResult<Vec<ActivityReplyVm>> {
        Err(UiError::not_wired("anilist_activity_replies"))
    }
    async fn anilist_like_activity(&self, _activity_id: u64, _like: bool) -> UiResult<()> {
        Err(UiError::not_wired("anilist_like_activity"))
    }
    async fn anilist_reply(&self, _activity_id: u64, _text: &str) -> UiResult<ActivityReplyVm> {
        Err(UiError::not_wired("anilist_reply"))
    }
    /// Post a status (TextActivity) on the viewer's AniList profile.
    async fn anilist_post_status(&self, _text: &str) -> UiResult<ActivityVm> {
        Err(UiError::not_wired("anilist_post_status"))
    }
    async fn anilist_delete_activity(&self, _activity_id: u64) -> UiResult<()> {
        Err(UiError::not_wired("anilist_delete_activity"))
    }
    /// Notifications page (`social.notifications.list`); `unread_only` filters client-side.
    async fn anilist_notifications(&self, _page: u32, _unread_only: bool) -> UiResult<NotificationPageVm> {
        Err(UiError::not_wired("anilist_notifications"))
    }
    /// `Viewer.unreadNotificationCount` (rail badge, polled every `Settings::social.notifications_poll_minutes`).
    async fn anilist_unread_count(&self) -> UiResult<u32> {
        Err(UiError::not_wired("anilist_unread_count"))
    }
    /// Reset AniList's unread count (`Page.notifications(resetNotificationCount: true)`).
    async fn anilist_mark_notifications_read(&self) -> UiResult<()> {
        Err(UiError::not_wired("anilist_mark_notifications_read"))
    }
    /// Reviews of a title (`details.reviews`), newest first.
    async fn anilist_reviews(&self, _media_id: &str, _page: u32) -> UiResult<Vec<ReviewVm>> {
        Err(UiError::not_wired("anilist_reviews"))
    }
    /// Full body of one review (summary comes with the list).
    async fn anilist_review(&self, _review_id: u64) -> UiResult<ReviewVm> {
        Err(UiError::not_wired("anilist_review"))
    }
}

pub type BackendRef = Arc<dyn Backend>;
