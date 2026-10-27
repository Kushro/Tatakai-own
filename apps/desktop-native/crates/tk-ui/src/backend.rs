//! The seam between the UI and the app. tk-ui only knows these view-model types and the [`Backend`]
//! trait; tk-app implements it over tk-core / tk-ext / tk-link. Strings are display-ready except
//! where an i18n key is noted.

use std::sync::Arc;

use async_trait::async_trait;

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
}

pub type BackendRef = Arc<dyn Backend>;
