//! Home / discover preferences and view-models (wave B, worker **b**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.1). `HomePrefs` is embedded in `Settings::home` (`SettingsAction::SetHome`); the view-models are
//! re-exported from `backend.rs`. Worker b owns this file (fields, bounds, tests); `model/mod.rs` stays frozen.

use serde::{Deserialize, Serialize};

use crate::backend::{MediaVm, TorrentLibraryItemVm};

/// Bounds of [`HomePrefs::reminder_minutes`].
pub const REMINDER_MIN: u16 = 0;
pub const REMINDER_MAX: u16 = 1440;

/// Mature (18+) content handling (`settings.privacy.mature-content`; classic `PrivacyPanel`): the only part of the
/// classic Privacy panel that did not depend on the own profile. Home, Search, Details and Manga (worker l) read
/// it. `id()` = i18n suffix `home.mature.<id>`, `data-mature`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatureContentVm {
    /// Show 18+ titles like any other.
    Show,
    /// Blur posters in rows and results until hovered / focused.
    Blur,
    /// Ask before opening an 18+ title (the Details gate of wave A, `details.adult-gate`).
    #[default]
    Warn,
}

impl MatureContentVm {
    pub const ALL: [MatureContentVm; 3] = [Self::Show, Self::Blur, Self::Warn];

    pub fn id(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Blur => "blur",
            Self::Warn => "warn",
        }
    }
}

/// `Settings::home`: which Home blocks show and the airing-calendar reminders (classic `Index.tsx` sections,
/// `CalendarPage` reminders).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomePrefs {
    pub show_latest_episodes: bool,
    pub show_top10: bool,
    pub show_upcoming: bool,
    pub show_most_favorite: bool,
    pub show_because_you_watched: bool,
    pub show_genre_cloud: bool,
    /// Load one more genre row each time the page end is reached (`home.infinite-genre-rows`).
    pub infinite_genre_rows: bool,
    /// Airing calendar block in Home / Library (`home.calendar.schedule`).
    pub show_calendar: bool,
    /// Snackbar / tray notice before a tracked show airs (`home.calendar.reminders`).
    pub airing_reminders: bool,
    /// Minutes before the air time, `0..=1440`.
    pub reminder_minutes: u16,
    /// "For you" recommendations with explanations (`home.recs.*`).
    pub show_recommendations: bool,
    /// Torrent activity banner (`home.torrent-activity`).
    pub show_torrent_activity: bool,
    /// 18+ handling across Home / Search / Details / Manga (`settings.privacy.mature-content`).
    pub mature_content: MatureContentVm,
}

impl Default for HomePrefs {
    fn default() -> Self {
        Self {
            show_latest_episodes: true,
            show_top10: true,
            show_upcoming: true,
            show_most_favorite: true,
            show_because_you_watched: true,
            show_genre_cloud: true,
            infinite_genre_rows: true,
            show_calendar: true,
            airing_reminders: false,
            reminder_minutes: 30,
            show_recommendations: true,
            show_torrent_activity: true,
            mature_content: MatureContentVm::Warn,
        }
    }
}

impl HomePrefs {
    pub fn sanitized(mut self) -> Self {
        self.reminder_minutes = self.reminder_minutes.clamp(REMINDER_MIN, REMINDER_MAX);
        self
    }
}

/// Top 10 tabs (classic `TopAnimeSection`). `id()` is the i18n suffix `home.top.<id>` and the `data-range` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TopRangeVm {
    #[default]
    Today,
    Week,
    Month,
}

impl TopRangeVm {
    pub const ALL: [TopRangeVm; 3] = [Self::Today, Self::Week, Self::Month];

    pub fn id(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
}

/// One recently aired episode (classic `LatestEpisodes`, `FollowedAnimeEpisodes`).
#[derive(Debug, Clone, PartialEq)]
pub struct LatestEpisodeVm {
    pub media: MediaVm,
    pub episode: u32,
    pub aired_unix: i64,
}

/// "Because you watched <seed>" row (classic `BecauseYouWatched`).
#[derive(Debug, Clone, PartialEq)]
pub struct BecauseVm {
    pub seed: MediaVm,
    pub items: Vec<MediaVm>,
}

/// One slot of the airing calendar (classic `CalendarPage`); `tracked` = in the user's "My shows".
#[derive(Debug, Clone, PartialEq)]
pub struct AiringSlotVm {
    pub media: MediaVm,
    pub episode: u32,
    pub at_unix: i64,
    pub tracked: bool,
}

/// A "For you" recommendation (classic `RecommendationsPage`): `score` 0.0..=1.0, `reasons` display-ready
/// ("Shares the genre Action with 4 titles you finished"), `atlas` = 2-D position on the Anime Atlas map.
#[derive(Debug, Clone, PartialEq)]
pub struct RecommendationVm {
    pub media: MediaVm,
    pub score: f32,
    pub reasons: Vec<String>,
    pub atlas: Option<(f32, f32)>,
}

/// Feedback on a recommendation (classic `recommendation_feedback`, local only in Next). `id()` = `data-feedback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecsFeedbackVm {
    NotInterested,
    Skip,
    Like,
}

impl RecsFeedbackVm {
    pub const ALL: [RecsFeedbackVm; 3] = [Self::NotInterested, Self::Skip, Self::Like];

    pub fn id(self) -> &'static str {
        match self {
            Self::NotInterested => "not_interested",
            Self::Skip => "skip",
            Self::Like => "like",
        }
    }
}

/// Taste profile built from the local history + lists (classic `useRecommendationEngine`): weights 0.0..=1.0.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TasteProfileVm {
    pub genres: Vec<(String, f32)>,
    pub studios: Vec<(String, f32)>,
    pub eras: Vec<(String, f32)>,
    pub themes: Vec<(String, f32)>,
    /// Titles the profile was built from.
    pub sample_size: u32,
    /// Share of past recommendations the user then watched, when enough feedback exists.
    pub accuracy: Option<f32>,
}

/// One point of the Anime Atlas (`home.recs.atlas`): normalized 0.0..=1.0 coordinates and a cluster label.
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasPointVm {
    pub media: MediaVm,
    pub x: f32,
    pub y: f32,
    pub cluster: String,
}

/// Torrent activity banner on Home (classic `TorrentSessionBanner`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TorrentActivityVm {
    pub active: u32,
    pub down_bps: u64,
    pub up_bps: u64,
    pub items: Vec<TorrentLibraryItemVm>,
}

/// Health of one metadata provider (Settings > Metadata, `settings.metadata.extra-providers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderHealthVm {
    /// `anilist`, `mal`, `shikimori`, `tenrai`, `kitsu`, `anizip`, `mangadex`, `mangaupdates`.
    pub id: String,
    /// In `Settings::provider_priority`.
    pub enabled: bool,
    /// Skipped right now after a transport / server failure (`MetadataRouter::cooling_down`).
    pub cooling_down: bool,
    /// Seconds left of the cool-down, when `cooling_down` (`settings.metadata.provider-status` "paused N min").
    pub cooling_secs: Option<u32>,
    /// `anime` / `manga`.
    pub kinds: Vec<String>,
    /// Last successful answer (unix seconds), `None` = never asked this session.
    pub last_ok_unix: Option<i64>,
    /// Last failure, display-ready (`None` = the last call succeeded).
    pub last_error: Option<String>,
    /// The provider's API no longer answers at all and the router drops it (plan 14 section 0.1 #13: "discarded with
    /// a note"); Settings shows it greyed out with the note.
    pub discontinued: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = HomePrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn reminder_minutes_are_clamped() {
        let p = HomePrefs { reminder_minutes: 9000, ..HomePrefs::default() }.sanitized();
        assert_eq!(p.reminder_minutes, REMINDER_MAX);
    }
}
