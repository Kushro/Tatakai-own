//! AniList "everything else" of a title (wave A, worker **c**; docs/redesign/15 section 4): relations,
//! characters with voice actors, staff, recommendations, trailer, next airing episode, studios, producers, tags,
//! synonyms, dates, duration, source, external links. Contract: [`MediaExtra`] is final; the body is a stub.
//!
//! Expected query: `Media(id:, idMal:, type:) { format source season seasonYear duration startDate { year month
//! day } endDate { ... } studios { edges { isMain node { name } } } countryOfOrigin meanScore popularity favourites
//! synonyms isAdult tags { name rank isMediaSpoiler } trailer { id site thumbnail } nextAiringEpisode { episode
//! airingAt } relations { edges { relationType node { ...mediaCard } } } characters(sort: ROLE, perPage: 25) {
//! edges { role voiceActors(language: JAPANESE) { ... } node { id name { full } image { medium } } } }
//! staff(perPage: 12) { edges { role node { id name { full } image { medium } } } } recommendations(perPage: 12,
//! sort: RATING_DESC) { nodes { mediaRecommendation { ...mediaCard } } } externalLinks { site url } }`.

use super::AniList;
use super::search::Person;
use crate::error::ProviderError;
use crate::model::{Media, MediaRef};

#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    pub media: Media,
    /// AniList `MediaRelation` (`PREQUEL`, `SEQUEL`, ...).
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRole {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
    /// `MAIN | SUPPORTING | BACKGROUND`
    pub role: String,
    pub voice_actors: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaExtra {
    pub format: Option<String>,
    pub source: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub duration_min: Option<u32>,
    /// `(year, month, day)` fuzzy dates.
    pub start_date: (Option<i32>, Option<u32>, Option<u32>),
    pub end_date: (Option<i32>, Option<u32>, Option<u32>),
    pub studios: Vec<String>,
    pub producers: Vec<String>,
    pub country: Option<String>,
    pub mean_score: Option<u32>,
    pub popularity: Option<u32>,
    pub favourites: Option<u32>,
    pub synonyms: Vec<String>,
    /// `(name, rank)`, spoiler tags excluded.
    pub tags: Vec<(String, u8)>,
    pub adult: bool,
    /// `(site, id, thumbnail)`
    pub trailer: Option<(String, String, Option<String>)>,
    /// `(episode, airing_at unix)`
    pub next_airing: Option<(u32, i64)>,
    pub relations: Vec<Relation>,
    pub characters: Vec<CharacterRole>,
    pub staff: Vec<Person>,
    pub recommendations: Vec<Media>,
    /// `(site, url)`
    pub external_links: Vec<(String, String)>,
}

impl AniList {
    /// Contract stub (worker c). `Ok(None)` when the title is unknown.
    pub async fn media_extra(&self, r: &MediaRef) -> Result<Option<MediaExtra>, ProviderError> {
        let _ = r;
        Err(ProviderError::Unsupported)
    }
}
