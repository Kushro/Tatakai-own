//! MyAnimeList REST v2 (wave A, worker **g**): `GET users/@me`, `GET users/@me/animelist?fields=list_status&
//! limit=1000&nsfw=true` (paged with `paging.next`), `PATCH anime/{id}/my_list_status`, `DELETE anime/{id}/
//! my_list_status`. Bearer token from `crate::lists::Token`. Contract: types final, bodies are stubs.

use super::Token;
use crate::error::ListError;

pub const API: &str = "https://api.myanimelist.net/v2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalUser {
    pub id: u64,
    pub name: String,
    pub picture: Option<String>,
}

/// One `data[]` row of the anime list.
#[derive(Debug, Clone, PartialEq)]
pub struct MalEntry {
    pub mal_id: u32,
    pub title: String,
    pub poster: Option<String>,
    pub episodes: Option<u32>,
    /// MAL `status` (`watching | completed | on_hold | dropped | plan_to_watch`).
    pub status: String,
    pub is_rewatching: bool,
    pub progress: u32,
    /// 0..=10
    pub score: u8,
    pub start_date: Option<String>,
    pub finish_date: Option<String>,
    pub num_times_rewatched: u32,
    pub comments: Option<String>,
    /// RFC 3339
    pub updated_at: String,
}

/// Fields of `PATCH my_list_status`; `None` = unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MalEntryInput {
    pub status: Option<String>,
    pub is_rewatching: Option<bool>,
    pub progress: Option<u32>,
    pub score: Option<u8>,
    pub comments: Option<String>,
}

pub struct Mal {
    client: reqwest::Client,
    api: String,
}

impl Mal {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, api: API.to_owned() }
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = api.into();
        self
    }

    /// Contract stub (worker g).
    pub async fn me(&self, token: &Token) -> Result<MalUser, ListError> {
        let _ = (token, &self.client, &self.api);
        Err(ListError::Unsupported)
    }
    /// Whole list, every status. Contract stub (worker g).
    pub async fn anime_list(&self, token: &Token) -> Result<Vec<MalEntry>, ListError> {
        let _ = token;
        Err(ListError::Unsupported)
    }
    /// Contract stub (worker g).
    pub async fn update(&self, token: &Token, mal_id: u32, input: &MalEntryInput) -> Result<MalEntry, ListError> {
        let _ = (token, mal_id, input);
        Err(ListError::Unsupported)
    }
    /// Contract stub (worker g).
    pub async fn delete(&self, token: &Token, mal_id: u32) -> Result<(), ListError> {
        let _ = (token, mal_id);
        Err(ListError::Unsupported)
    }
}
