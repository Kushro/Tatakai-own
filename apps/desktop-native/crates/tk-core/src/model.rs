//! Domain model. Semantics mirror the Kotlin `app.tatakai.next.model` package (docs/redesign/04 section 3.2).
//! JSON field names are the Kotlin (camelCase) ones so the contract fixtures in `contracts/fixtures/` compare 1:1.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MediaKind {
    Anime,
    Manga,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anime => "ANIME",
            Self::Manga => "MANGA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FeedKind {
    Trending,
    Popular,
    TopRated,
    Airing,
    Upcoming,
    Seasonal,
    RecentlyCompleted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaRef {
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
    pub kitsu_id: Option<u32>,
}

impl MediaRef {
    pub fn anilist(id: u32) -> Self {
        Self { anilist_id: Some(id), ..Self::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.anilist_id.is_none() && self.mal_id.is_none() && self.kitsu_id.is_none()
    }
}

/// `media_ref` is Kotlin's `ref` (a Rust keyword); the JSON name stays `ref`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(rename = "ref")]
    pub media_ref: MediaRef,
    pub kind: MediaKind,
    pub title: String,
    pub titles: BTreeMap<String, String>,
    pub cover: Option<String>,
    pub banner: Option<String>,
    pub color: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Vec<String>,
    pub episodes: Option<u32>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub year: Option<i32>,
    pub score: Option<u32>,
    pub source_provider: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub number: u32,
    pub title: Option<String>,
    pub image: Option<String>,
    pub air_date: Option<String>,
    #[serde(default)]
    pub is_filler: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedPage {
    pub items: Vec<Media>,
    pub page: u32,
    pub has_next: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub url: String,
    pub lang: String,
    pub label: String,
    pub format: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Container {
    Hls,
    Mp4,
    Dash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamSource {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub container: Container,
    pub quality: Option<String>,
    pub provider: String,
    pub extension_id: String,
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
}

/// One watched episode. Keys follow the legacy web app so both apps share one Supabase history (E9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// `String(anilistId)` or `"mal:<malId>"`.
    pub anime_id: String,
    pub title: String,
    pub poster: Option<String>,
    /// `"<anime_id>?ep=<episode>"`.
    pub episode_id: String,
    pub episode: u32,
    pub progress_sec: u32,
    pub duration_sec: Option<u32>,
    pub completed: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub watched_at: OffsetDateTime,
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
}

impl HistoryEntry {
    /// Build an entry with the E9 key format and the 90 % completion rule.
    /// Returns `None` when the reference has neither an AniList nor a MAL id (Kitsu ids are never written).
    pub fn for_playback(
        media_ref: &MediaRef,
        title: &str,
        poster: Option<&str>,
        episode: u32,
        progress_sec: u32,
        duration_sec: Option<u32>,
        watched_at: OffsetDateTime,
    ) -> Option<Self> {
        let anime_id = Self::anime_key(media_ref)?;
        Some(Self {
            episode_id: Self::episode_key(&anime_id, episode),
            anime_id,
            title: title.to_owned(),
            poster: poster.map(str::to_owned),
            episode,
            progress_sec,
            duration_sec,
            completed: Self::is_completed(progress_sec, duration_sec),
            watched_at,
            anilist_id: media_ref.anilist_id,
            mal_id: media_ref.mal_id,
        })
    }

    pub fn anime_key(r: &MediaRef) -> Option<String> {
        match (r.anilist_id, r.mal_id) {
            (Some(a), _) if a > 0 => Some(a.to_string()),
            (_, Some(m)) if m > 0 => Some(format!("mal:{m}")),
            _ => None,
        }
    }

    pub fn episode_key(anime_id: &str, episode: u32) -> String {
        format!("{anime_id}?ep={episode}")
    }

    /// `duration > 0 && progress / duration >= 0.9`
    pub fn is_completed(progress_sec: u32, duration_sec: Option<u32>) -> bool {
        matches!(duration_sec, Some(d) if d > 0 && f64::from(progress_sec) / f64::from(d) >= 0.9)
    }

    /// Merge rule for local vs remote rows: newest `watched_at` wins, tie -> higher progress.
    pub fn wins_over(&self, other: &Self) -> bool {
        self.watched_at > other.watched_at
            || (self.watched_at == other.watched_at && self.progress_sec > other.progress_sec)
    }
}

/// "Continue watching": drop completed rows, keep the most recent episode per anime, newest first.
pub fn continue_watching(mut rows: Vec<HistoryEntry>, limit: usize) -> Vec<HistoryEntry> {
    rows.sort_by(|a, b| b.watched_at.cmp(&a.watched_at));
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|r| !r.completed)
        .filter(|r| seen.insert(r.anime_id.clone()))
        .take(limit)
        .collect()
}

/// Extension manifest summary shown by the UI (`InstalledExtension` in Kotlin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledExtension {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub permissions: Vec<String>,
    pub compat: Compat,
    /// Human-readable reason when `compat` is not `Full` (shown in the Extensions screen).
    #[serde(default)]
    pub compat_reason: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Compat {
    Full,
    Partial,
    Unsupported,
}
