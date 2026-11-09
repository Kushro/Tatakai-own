//! Authenticated AniList calls for the lists (wave A, worker **g**; docs/redesign/15 section 4): `Viewer`,
//! `MediaListCollection(userId, type: ANIME)`, `SaveMediaListEntry` / `DeleteMediaListEntry`,
//! `ToggleFavourite`. The bearer token comes from `crate::lists`; requests go through the same `post` with an
//! `Authorization` header (add a `post_auth` here). Contract: types final, bodies are stubs.

use super::AniList;
use crate::error::ListError;
use crate::model::Media;

/// `Viewer { id name avatar { medium } siteUrl }`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub id: u64,
    pub name: String,
    pub avatar: Option<String>,
    pub site_url: Option<String>,
}

/// One `MediaList` entry. `status` is AniList's (`CURRENT | PLANNING | COMPLETED | DROPPED | PAUSED | REPEATING`);
/// `score` on the viewer's `POINT_100` scale (ask `scoreFormat` and convert).
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteEntry {
    pub id: u64,
    pub media: Media,
    pub status: String,
    pub progress: u32,
    pub score_100: Option<u8>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub repeats: u32,
    pub notes: Option<String>,
    pub updated_at: i64,
}

/// Fields of `SaveMediaListEntry`; `None` = unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EntryInput {
    pub status: Option<String>,
    pub progress: Option<u32>,
    pub score_100: Option<u8>,
    pub notes: Option<String>,
}

impl AniList {
    /// Contract stub (worker g).
    pub async fn viewer(&self, token: &str) -> Result<Viewer, ListError> {
        let _ = token;
        Err(ListError::Unsupported)
    }
    /// Every anime entry of the viewer, all statuses. Contract stub (worker g).
    pub async fn media_list(&self, token: &str, user_id: u64) -> Result<Vec<RemoteEntry>, ListError> {
        let _ = (token, user_id);
        Err(ListError::Unsupported)
    }
    /// Create / update; returns the saved entry. Contract stub (worker g).
    pub async fn save_entry(&self, token: &str, anilist_media_id: u32, input: &EntryInput) -> Result<RemoteEntry, ListError> {
        let _ = (token, anilist_media_id, input);
        Err(ListError::Unsupported)
    }
    /// Contract stub (worker g).
    pub async fn delete_entry(&self, token: &str, entry_id: u64) -> Result<(), ListError> {
        let _ = (token, entry_id);
        Err(ListError::Unsupported)
    }
    /// `ToggleFavourite(animeId:)`. Contract stub (worker g).
    pub async fn toggle_favourite(&self, token: &str, anilist_media_id: u32) -> Result<(), ListError> {
        let _ = (token, anilist_media_id);
        Err(ListError::Unsupported)
    }
}
