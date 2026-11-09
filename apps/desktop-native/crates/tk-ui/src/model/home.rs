//! Home / discover preferences and view-models (wave B, worker **b**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.1). `HomePrefs` is embedded in `Settings::home` (`SettingsAction::SetHome`); the view-models are
//! re-exported from `backend.rs`. Worker b owns this file (fields, bounds, tests); `model/mod.rs` stays frozen.

use serde::{Deserialize, Serialize};

use crate::backend::{MediaVm, TorrentLibraryItemVm};
use crate::model::{PROVIDERS, PaletteInfo};

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
    /// Metadata providers switched off in Settings > Metadata (`settings.metadata.extra-providers`; ids of
    /// `model::PROVIDERS`). Kept here (worker b's struct) instead of a new top-level `Settings` field so the closed
    /// `model/mod.rs` does not change; [`HomePrefs::enabled_providers`] is what the metadata router receives.
    pub disabled_providers: Vec<String>,
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
            disabled_providers: Vec::new(),
        }
    }
}

impl HomePrefs {
    pub fn sanitized(mut self) -> Self {
        self.reminder_minutes = self.reminder_minutes.clamp(REMINDER_MIN, REMINDER_MAX);
        let mut off: Vec<String> = Vec::new();
        for p in &self.disabled_providers {
            if PROVIDERS.contains(&p.as_str()) && !off.contains(p) {
                off.push(p.clone());
            }
        }
        // At least one provider stays on: switching everything off would leave the app without metadata.
        if PROVIDERS.iter().all(|p| off.iter().any(|o| o == p)) {
            off.retain(|p| p != PROVIDERS[0]);
        }
        self.disabled_providers = off;
        self
    }

    /// `priority` without the disabled providers: the list `tk_core::MetadataRouter::set_priority` receives.
    pub fn enabled_providers(&self, priority: &[String]) -> Vec<String> {
        priority
            .iter()
            .filter(|p| !self.is_provider_disabled(p))
            .cloned()
            .collect()
    }

    pub fn is_provider_disabled(&self, id: &str) -> bool {
        self.disabled_providers.iter().any(|p| p == id)
    }

    /// Switch one provider on / off (sanitized: the last enabled one cannot be switched off).
    pub fn with_provider_enabled(mut self, id: &str, enabled: bool) -> Self {
        self.disabled_providers.retain(|p| p != id);
        if !enabled {
            self.disabled_providers.push(id.to_owned());
        }
        self.sanitized()
    }

    /// 18+ titles may appear in mixed rows (Top 10, latest episodes, recommendations) only with `Show`.
    pub fn adult_in_rows(&self) -> bool {
        self.mature_content == MatureContentVm::Show
    }

    /// 18+ titles may appear where they are the point (the `Hentai` genre row, Search with the 18+ filter): `Show`
    /// shows them, `Blur` shows them blurred, `Warn` hides them.
    pub fn adult_where_asked(&self) -> bool {
        self.mature_content != MatureContentVm::Warn
    }

    /// Posters of 18+ results are blurred (`data-blurred`).
    pub fn blur_adult(&self) -> bool {
        self.mature_content == MatureContentVm::Blur
    }
}

/// Home blocks with an on / off switch in Settings > Home & discover, in Home order. The id is the `HomePrefs` field
/// name, the i18n suffix `settings.home.block.<id>` and `data-home-block`.
pub const HOME_BLOCKS: [&str; 10] = [
    "show_latest_episodes",
    "show_top10",
    "show_upcoming",
    "show_most_favorite",
    "show_because_you_watched",
    "show_genre_cloud",
    "infinite_genre_rows",
    "show_calendar",
    "show_recommendations",
    "show_torrent_activity",
];

impl HomePrefs {
    fn block_mut(&mut self, id: &str) -> Option<&mut bool> {
        Some(match id {
            "show_latest_episodes" => &mut self.show_latest_episodes,
            "show_top10" => &mut self.show_top10,
            "show_upcoming" => &mut self.show_upcoming,
            "show_most_favorite" => &mut self.show_most_favorite,
            "show_because_you_watched" => &mut self.show_because_you_watched,
            "show_genre_cloud" => &mut self.show_genre_cloud,
            "infinite_genre_rows" => &mut self.infinite_genre_rows,
            "show_calendar" => &mut self.show_calendar,
            "show_recommendations" => &mut self.show_recommendations,
            "show_torrent_activity" => &mut self.show_torrent_activity,
            _ => return None,
        })
    }

    /// Value of one of [`HOME_BLOCKS`] (`false` for an unknown id).
    pub fn block(&self, id: &str) -> bool {
        self.clone().block_mut(id).is_some_and(|v| *v)
    }

    /// The prefs with one block switched (unknown ids change nothing).
    pub fn with_block(mut self, id: &str, on: bool) -> Self {
        if let Some(v) = self.block_mut(id) {
            *v = on;
        }
        self
    }
}

/// Every palette of `packages/tokens/tokens.json` (wave B, `settings.appearance.more-palettes`): the 10 curated ones of
/// 01-design-system plus 10 ported from the classic themes. `model/mod.rs::PALETTES` takes this list (integrator);
/// Settings > Appearance draws one swatch per entry (`data-palette`). Labels: `palette.<id>`.
pub const ALL_PALETTES: [PaletteInfo; 20] = [
    PaletteInfo {
        id: "tatakai-violet",
        seed: "#7C4DFF",
    },
    PaletteInfo {
        id: "midnight",
        seed: "#3D5AFE",
    },
    PaletteInfo {
        id: "sakura",
        seed: "#E86A92",
    },
    PaletteInfo {
        id: "matcha",
        seed: "#5E9E6E",
    },
    PaletteInfo {
        id: "ocean",
        seed: "#0091C2",
    },
    PaletteInfo {
        id: "neon-tokyo",
        seed: "#FF2E88",
    },
    PaletteInfo {
        id: "blood-moon",
        seed: "#C62828",
    },
    PaletteInfo {
        id: "royal-gold",
        seed: "#C9A227",
    },
    PaletteInfo {
        id: "aurora",
        seed: "#00BFA5",
    },
    PaletteInfo {
        id: "mono",
        seed: "#808080",
    },
    PaletteInfo {
        id: "cherry-blossom",
        seed: "#EC4899",
    },
    PaletteInfo {
        id: "deep-ocean",
        seed: "#1E5AA8",
    },
    PaletteInfo {
        id: "cyberpunk",
        seed: "#C6D300",
    },
    PaletteInfo {
        id: "zen-garden",
        seed: "#2E8B57",
    },
    PaletteInfo {
        id: "solar",
        seed: "#F59E0B",
    },
    PaletteInfo {
        id: "caffeine",
        seed: "#8B5A2B",
    },
    PaletteInfo {
        id: "dark-matter",
        seed: "#5B21B6",
    },
    PaletteInfo {
        id: "frostbite",
        seed: "#38BDF8",
    },
    PaletteInfo {
        id: "synthwave",
        seed: "#D946EF",
    },
    PaletteInfo {
        id: "vampire",
        seed: "#8B0000",
    },
];

/// `data-blurred` of a card: only 18+ cards in `Blur` mode.
pub fn blurred(prefs: &HomePrefs, adult: bool) -> bool {
    adult && prefs.blur_adult()
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

/// A "For you" recommendation (classic `RecommendationsPage`): `score` 0.0..=1.0, `reasons` are tokens
/// `<kind>:<value>` (`genre:Action`, `studio:MAPPA`, `era:2010s`, `tag:Time Skip`, `score:86`; `tk_core::recs::Reason`)
/// that the UI renders with `recs.reason.<kind>` (i18n stays in the UI), `atlas` = 2-D position on the Anime Atlas map.
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

/// Values of `data-provider-status` (Settings > Metadata, `settings.metadata.provider-status`).
pub const PROVIDER_STATUSES: [&str; 5] = ["ok", "error", "cooling", "off", "discontinued"];

impl ProviderHealthVm {
    /// `discontinued` > `off` (switched off) > `cooling` (router pause) > `error` (last call failed) > `ok`.
    pub fn status(&self) -> &'static str {
        if self.discontinued {
            "discontinued"
        } else if !self.enabled {
            "off"
        } else if self.cooling_down {
            "cooling"
        } else if self.last_error.is_some() {
            "error"
        } else {
            "ok"
        }
    }

    /// Minutes left of the pause, rounded up (at least 1 while cooling).
    pub fn cooling_minutes(&self) -> Option<u32> {
        self.cooling_secs
            .filter(|_| self.cooling_down)
            .map(|s| s.div_ceil(60).max(1))
    }
}

/// `last_error` value meaning "this build has no MAL client id" (Settings explains it with
/// `settings.metadata.status.no_client_id`).
pub const NO_CLIENT_ID: &str = "no_client_id";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = HomePrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn providers_can_be_disabled_but_not_all() {
        let p = HomePrefs::default()
            .with_provider_enabled("mal", false)
            .with_provider_enabled("bogus", false);
        assert_eq!(p.disabled_providers, vec!["mal".to_owned()]);
        let prio: Vec<String> = PROVIDERS.iter().map(|s| (*s).to_owned()).collect();
        let on = p.enabled_providers(&prio);
        assert_eq!(on.len(), PROVIDERS.len() - 1);
        assert!(!on.contains(&"mal".to_owned()));
        let all_off = PROVIDERS.iter().fold(HomePrefs::default(), |acc, id| {
            acc.with_provider_enabled(id, false)
        });
        assert_eq!(all_off.enabled_providers(&prio), vec![PROVIDERS[0].to_owned()]);
        assert!(p.with_provider_enabled("mal", true).disabled_providers.is_empty());
    }

    #[test]
    fn mature_modes() {
        let mut p = HomePrefs::default();
        assert!(!p.adult_in_rows() && !p.adult_where_asked() && !p.blur_adult());
        p.mature_content = MatureContentVm::Blur;
        assert!(!p.adult_in_rows() && p.adult_where_asked() && p.blur_adult());
        p.mature_content = MatureContentVm::Show;
        assert!(p.adult_in_rows() && p.adult_where_asked() && !p.blur_adult());
    }

    #[test]
    fn provider_status_order() {
        let mut h = ProviderHealthVm {
            id: "mal".into(),
            enabled: true,
            cooling_down: false,
            cooling_secs: None,
            kinds: vec![],
            last_ok_unix: None,
            last_error: None,
            discontinued: false,
        };
        assert_eq!(h.status(), "ok");
        h.last_error = Some("x".into());
        assert_eq!(h.status(), "error");
        h.cooling_down = true;
        h.cooling_secs = Some(61);
        assert_eq!((h.status(), h.cooling_minutes()), ("cooling", Some(2)));
        h.enabled = false;
        assert_eq!(h.status(), "off");
        h.discontinued = true;
        assert_eq!(h.status(), "discontinued");
        assert!(PROVIDER_STATUSES.contains(&h.status()));
    }

    #[test]
    fn old_files_without_new_fields_load() {
        let p: HomePrefs = serde_json::from_str(r#"{"show_top10":false}"#).unwrap();
        assert!(!p.show_top10);
        assert!(p.disabled_providers.is_empty());
        assert_eq!(p.mature_content, MatureContentVm::Warn);
    }

    #[test]
    fn blocks_round_trip() {
        let mut p = HomePrefs::default();
        for id in HOME_BLOCKS {
            assert!(p.block(id), "{id} defaults on");
            p = p.with_block(id, false);
            assert!(!p.block(id), "{id}");
        }
        assert!(!p.block("bogus"));
        assert_eq!(p.clone().with_block("bogus", true), p);
    }

    #[test]
    fn palettes_match_tokens_json() {
        let raw = include_str!("../../../../../../packages/tokens/tokens.json");
        let doc: serde_json::Value = serde_json::from_str(raw).unwrap();
        let palettes = doc["palettes"].as_object().unwrap();
        assert_eq!(palettes.len(), ALL_PALETTES.len());
        assert!(ALL_PALETTES.len() >= 16);
        for p in ALL_PALETTES {
            assert_eq!(
                palettes.get(p.id).and_then(|v| v["seed"].as_str()),
                Some(p.seed),
                "{}",
                p.id
            );
        }
        assert_eq!(doc["defaultPalette"].as_str(), Some(ALL_PALETTES[0].id));
    }

    #[test]
    fn only_adult_cards_blur_and_only_in_blur_mode() {
        let mut p = HomePrefs::default();
        assert!(!blurred(&p, true));
        p.mature_content = MatureContentVm::Blur;
        assert!(blurred(&p, true) && !blurred(&p, false));
    }

    #[test]
    fn reminder_minutes_are_clamped() {
        let p = HomePrefs {
            reminder_minutes: 9000,
            ..HomePrefs::default()
        }
        .sanitized();
        assert_eq!(p.reminder_minutes, REMINDER_MAX);
    }
}
