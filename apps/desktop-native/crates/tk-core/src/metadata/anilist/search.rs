//! AniList filtered search, studio pages and character pages (wave A, worker **a**; docs/redesign/15 section 4).
//! Contract: the variables struct and the three entry points are final; bodies are stubs (`Unsupported`).
//!
//! Expected implementation: one `Page(page, perPage) { pageInfo { ... } media(type, search, genre_in, tag_in,
//! format_in, status_in, season, seasonYear, startDate_greater/lesser, episodes_greater/lesser,
//! averageScore_greater/lesser, sort, isAdult) { ...mediaCard } }` query built from [`SearchVars`] (reuse
//! `CARD_FRAGMENT` / `map_anilist_page` of `mod.rs`); `Studio(search:) { media(page, sort) }` for studios;
//! `Character(id:) { name { full native alternative } image { large } description gender age dateOfBirth
//! bloodType favourites media(page, perPage, sort: POPULARITY_DESC) { edges { characterRole voiceActors(language)
//! { id name { full } image { medium } languageV2 } node { ...mediaCard } } } }` for characters.

use serde_json::Value;

use super::AniList;
use crate::error::ProviderError;
use crate::model::{FeedPage, MediaKind};

/// Sort orders accepted by `search_filtered` (mirror of the UI `SearchSortVm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchSort {
    #[default]
    Relevance,
    Popularity,
    Score,
    Trending,
    Newest,
    Title,
}

/// Variables of the filtered search; `None` / empty = not filtered. Strings use AniList's own enum spellings
/// (`format` `TV | MOVIE | ...`, `status` `RELEASING | FINISHED | NOT_YET_RELEASED | CANCELLED | HIATUS`, `season`
/// `WINTER | SPRING | SUMMER | FALL`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchVars {
    pub kind: Option<MediaKind>,
    pub query: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub episodes_min: Option<u32>,
    pub episodes_max: Option<u32>,
    pub score_min: Option<u32>,
    pub score_max: Option<u32>,
    pub sort: SearchSort,
    pub adult: bool,
    pub page: u32,
    pub per_page: u32,
}

/// A character page as AniList returns it (mapped to the UI `CharacterVm` by tk-app).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Character {
    pub id: u32,
    pub name: String,
    pub native_name: Option<String>,
    pub alternative_names: Vec<String>,
    pub image: Option<String>,
    pub description: Option<String>,
    pub gender: Option<String>,
    pub age: Option<String>,
    /// `(year, month, day)` as AniList's fuzzy date (any part may be missing).
    pub birthday: (Option<i32>, Option<u32>, Option<u32>),
    pub blood_type: Option<String>,
    pub favourites: Option<u32>,
    pub appearances: Vec<CharacterAppearance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharacterAppearance {
    pub media: crate::model::Media,
    /// `MAIN | SUPPORTING | BACKGROUND`
    pub role: String,
    pub voice_actors: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
    /// Voice actor language (`Japanese`) or staff role (`Director`).
    pub role: Option<String>,
}

impl AniList {
    /// GraphQL variables for `SearchVars` (pure, testable). Contract stub: only `page` / `perPage` / `type`.
    pub fn search_vars(v: &SearchVars) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("page".into(), v.page.max(1).into());
        m.insert("perPage".into(), v.per_page.clamp(1, 50).into());
        if let Some(k) = v.kind {
            m.insert("type".into(), k.as_str().into());
        }
        Value::Object(m)
    }

    /// Contract stub (worker a).
    pub async fn search_filtered(&self, v: &SearchVars) -> Result<FeedPage, ProviderError> {
        let _ = v;
        Err(ProviderError::Unsupported)
    }

    /// Titles of a studio / producer by name, popularity first. Contract stub (worker a).
    pub async fn studio(&self, name: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let _ = (name, page);
        Err(ProviderError::Unsupported)
    }

    /// Character page by AniList character id. `Ok(None)` when unknown. Contract stub (worker a).
    pub async fn character(&self, id: u32) -> Result<Option<Character>, ProviderError> {
        let _ = id;
        Err(ProviderError::Unsupported)
    }
}
