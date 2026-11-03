//! The seam between the UI and the app. tk-ui only knows these view-model types and the [`Backend`]
//! trait; tk-app implements it over tk-core / tk-ext / tk-link. Strings are display-ready except
//! where an i18n key is noted.

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
}

pub type BackendRef = Arc<dyn Backend>;
