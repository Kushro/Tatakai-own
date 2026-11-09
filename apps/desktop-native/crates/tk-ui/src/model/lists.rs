//! List (AniList / MyAnimeList) preferences (wave A, worker **g**; docs/redesign/15-parity-wave-a-contract.md
//! section 3). Embedded in `Settings::lists`. Worker g owns this file (fields, bounds, tests); `model/mod.rs`
//! stays frozen. The list view-models (`ListEntryVm`, `ListStatusVm`, ...) live in `backend.rs`.

use serde::{Deserialize, Serialize};

/// The two list services of wave A. Persisted as `"anilist"` / `"mal"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ListServiceVm {
    AniList,
    Mal,
}

impl ListServiceVm {
    pub const ALL: [ListServiceVm; 2] = [Self::AniList, Self::Mal];

    /// i18n suffix (`lists.service.<id>`), `data-service` value and the key of `Backend::list_*` calls.
    pub fn id(self) -> &'static str {
        match self {
            Self::AniList => "anilist",
            Self::Mal => "mal",
        }
    }

    /// Brand name (never translated).
    pub fn label(self) -> &'static str {
        match self {
            Self::AniList => "AniList",
            Self::Mal => "MyAnimeList",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }
}

/// Bounds of `ListPrefsVm::progress_at`.
pub const PROGRESS_AT_MIN: u8 = 50;
pub const PROGRESS_AT_MAX: u8 = 100;

/// `Settings::lists`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ListPrefsVm {
    /// Advance the list progress when an episode is watched (`Backend::record_progress` hook).
    pub auto_progress: bool,
    /// Percent of the duration after which an episode counts as watched for the lists, `50..=100`.
    pub progress_at: u8,
    /// The service whose entry the details button shows first when both are connected.
    pub primary: Option<ListServiceVm>,
    /// Push local changes to every connected service (false = read-only mirror).
    pub write_back: bool,
}

impl Default for ListPrefsVm {
    fn default() -> Self {
        Self { auto_progress: true, progress_at: 90, primary: None, write_back: true }
    }
}

impl ListPrefsVm {
    pub fn sanitized(mut self) -> Self {
        self.progress_at = self.progress_at.clamp(PROGRESS_AT_MIN, PROGRESS_AT_MAX);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_sanitize_and_progress_is_clamped() {
        let d = ListPrefsVm::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(ListPrefsVm { progress_at: 1, ..d.clone() }.sanitized().progress_at, PROGRESS_AT_MIN);
        assert_eq!(ListPrefsVm { progress_at: 255, ..d }.sanitized().progress_at, PROGRESS_AT_MAX);
    }

    #[test]
    fn service_ids_round_trip() {
        for s in ListServiceVm::ALL {
            assert_eq!(ListServiceVm::from_id(s.id()), Some(s));
            assert_eq!(serde_json::to_string(&s).unwrap(), format!("\"{}\"", s.id()));
        }
        assert_eq!(ListServiceVm::from_id("kitsu"), None);
    }
    // ---- F6 (wave A gaps) ----

    #[test]
    fn labels_and_primary_service_wire() {
        assert_eq!(ListServiceVm::AniList.label(), "AniList");
        assert_eq!(ListServiceVm::Mal.label(), "MyAnimeList");
        let p: ListPrefsVm = serde_json::from_str(r#"{"primary":"mal","progress_at":75}"#).unwrap();
        assert_eq!(p, ListPrefsVm { primary: Some(ListServiceVm::Mal), progress_at: 75, ..ListPrefsVm::default() });
        assert!(serde_json::from_str::<ListPrefsVm>(r#"{"primary":"AniList"}"#).is_err(), "ids are lower case");
        let inside = ListPrefsVm { progress_at: PROGRESS_AT_MIN, auto_progress: false, write_back: false, primary: Some(ListServiceVm::AniList) };
        assert_eq!(inside.clone().sanitized(), inside);
    }
}
