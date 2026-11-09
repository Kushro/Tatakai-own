//! MyAnimeList REST v2 (wave A, worker **g**): `GET users/@me`, `GET users/@me/animelist?fields=list_status&
//! limit=1000&nsfw=true` (paged with `paging.next`), `PATCH anime/{id}/my_list_status`, `DELETE anime/{id}/
//! my_list_status`. Bearer token from `crate::lists::Token`. Contract: types final.

use serde_json::Value;

use super::Token;
use crate::error::ListError;

/// `fields` of the list request (MAL only returns what is asked for).
pub const LIST_FIELDS: &str = "list_status{status,score,num_episodes_watched,is_rewatching,updated_at,start_date,finish_date,num_times_rewatched,comments},num_episodes";
/// Safety net against a paging loop.
const MAX_PAGES: usize = 50;

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
        Self {
            client,
            api: API.to_owned(),
        }
    }

    pub fn with_api(mut self, api: impl Into<String>) -> Self {
        self.api = api.into();
        self
    }

    fn check(status: u16) -> Result<(), ListError> {
        match status {
            200..=299 => Ok(()),
            401 | 403 => Err(ListError::Unauthorized),
            429 => Err(ListError::RateLimited),
            s => Err(ListError::Http(s)),
        }
    }

    async fn get_json(&self, token: &Token, url: &str, query: &[(&str, &str)]) -> Result<Value, ListError> {
        let resp = self
            .client
            .get(url)
            .bearer_auth(&token.access_token)
            .query(query)
            .send()
            .await?;
        Self::check(resp.status().as_u16())?;
        resp.json()
            .await
            .map_err(|e| ListError::Parse(e.without_url().to_string()))
    }

    pub async fn me(&self, token: &Token) -> Result<MalUser, ListError> {
        let v = self
            .get_json(
                token,
                &format!("{}/users/@me", self.api),
                &[("fields", "picture")],
            )
            .await?;
        parse_user(&v).ok_or_else(|| ListError::Parse("users/@me".into()))
    }

    /// Whole list, every status (follows `paging.next`).
    pub async fn anime_list(&self, token: &Token) -> Result<Vec<MalEntry>, ListError> {
        let mut out = Vec::new();
        let first = format!("{}/users/@me/animelist", self.api);
        let mut page = self
            .get_json(
                token,
                &first,
                &[("fields", LIST_FIELDS), ("limit", "1000"), ("nsfw", "true")],
            )
            .await?;
        for _ in 0..MAX_PAGES {
            out.extend(
                page["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(parse_list_row),
            );
            let Some(next) = page["paging"]["next"].as_str().map(str::to_owned) else {
                break;
            };
            // `next` is absolute and already carries the query.
            page = self.get_json(token, &next, &[]).await?;
        }
        Ok(out)
    }

    pub async fn update(
        &self,
        token: &Token,
        mal_id: u32,
        input: &MalEntryInput,
    ) -> Result<MalEntry, ListError> {
        let form = update_form(input);
        let resp = self
            .client
            .patch(format!("{}/anime/{mal_id}/my_list_status", self.api))
            .bearer_auth(&token.access_token)
            .form(&form)
            .send()
            .await?;
        Self::check(resp.status().as_u16())?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| ListError::Parse(e.without_url().to_string()))?;
        parse_status(mal_id, String::new(), None, None, &v)
            .ok_or_else(|| ListError::Parse("my_list_status".into()))
    }

    pub async fn delete(&self, token: &Token, mal_id: u32) -> Result<(), ListError> {
        let resp = self
            .client
            .delete(format!("{}/anime/{mal_id}/my_list_status", self.api))
            .bearer_auth(&token.access_token)
            .send()
            .await?;
        match resp.status().as_u16() {
            // Not in the list any more: the goal is reached.
            404 => Ok(()),
            s => Self::check(s),
        }
    }
}

/// `users/@me` -> [`MalUser`].
pub fn parse_user(v: &Value) -> Option<MalUser> {
    Some(MalUser {
        id: v["id"].as_u64()?,
        name: v["name"].as_str()?.to_owned(),
        picture: v["picture"].as_str().map(str::to_owned),
    })
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

fn u32_of(v: &Value) -> u32 {
    v.as_u64().map_or(0, |n| n.min(u64::from(u32::MAX)) as u32)
}

/// A `list_status` object (+ the node fields the caller knows) -> [`MalEntry`].
fn parse_status(
    mal_id: u32,
    title: String,
    poster: Option<String>,
    episodes: Option<u32>,
    ls: &Value,
) -> Option<MalEntry> {
    Some(MalEntry {
        mal_id,
        title,
        poster,
        episodes,
        status: ls["status"].as_str()?.to_owned(),
        is_rewatching: ls["is_rewatching"].as_bool().unwrap_or(false),
        progress: u32_of(&ls["num_episodes_watched"]),
        score: u32_of(&ls["score"]).min(10) as u8,
        start_date: opt_str(&ls["start_date"]),
        finish_date: opt_str(&ls["finish_date"]),
        num_times_rewatched: u32_of(&ls["num_times_rewatched"]),
        comments: opt_str(&ls["comments"]),
        updated_at: ls["updated_at"].as_str().unwrap_or_default().to_owned(),
    })
}

/// One `data[]` row (`{ node, list_status }`) of the anime list.
pub fn parse_list_row(row: &Value) -> Option<MalEntry> {
    let node = &row["node"];
    let mal_id = u32::try_from(node["id"].as_u64()?).ok()?;
    let poster = node["main_picture"]["large"]
        .as_str()
        .or_else(|| node["main_picture"]["medium"].as_str())
        .map(str::to_owned);
    let episodes = node["num_episodes"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n > 0);
    parse_status(
        mal_id,
        node["title"].as_str().unwrap_or_default().to_owned(),
        poster,
        episodes,
        &row["list_status"],
    )
}

/// Form fields of `PATCH my_list_status` (only the changed ones).
pub fn update_form(input: &MalEntryInput) -> Vec<(&'static str, String)> {
    let mut form = Vec::new();
    if let Some(s) = &input.status {
        form.push(("status", s.clone()));
    }
    if let Some(r) = input.is_rewatching {
        form.push(("is_rewatching", r.to_string()));
    }
    if let Some(p) = input.progress {
        form.push(("num_watched_episodes", p.to_string()));
    }
    if let Some(s) = input.score {
        form.push(("score", s.min(10).to_string()));
    }
    if let Some(c) = &input.comments {
        form.push(("comments", c.clone()));
    }
    form
}

/// MAL `updated_at` (RFC 3339) -> unix seconds; 0 when unknown.
pub fn updated_unix(updated_at: &str) -> i64 {
    time::OffsetDateTime::parse(updated_at, &time::format_description::well_known::Rfc3339)
        .map_or(0, |t| t.unix_timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_list_rows_and_user() {
        let row = json!({ "node": { "id": 52991, "title": "Frieren", "main_picture": { "medium": "m", "large": "l" }, "num_episodes": 28 },
            "list_status": { "status": "watching", "score": 9, "num_episodes_watched": 5, "is_rewatching": false,
                             "updated_at": "2024-03-01T10:00:00+00:00", "start_date": "2024-01-02", "comments": "" } });
        let e = parse_list_row(&row).unwrap();
        assert_eq!(
            (e.mal_id, e.title.as_str(), e.poster.as_deref(), e.episodes),
            (52991, "Frieren", Some("l"), Some(28))
        );
        assert_eq!(
            (e.status.as_str(), e.score, e.progress, e.is_rewatching),
            ("watching", 9, 5, false)
        );
        assert_eq!(
            (
                e.start_date.as_deref(),
                e.finish_date.as_deref(),
                e.comments.as_deref()
            ),
            (Some("2024-01-02"), None, None)
        );
        assert_eq!(updated_unix(&e.updated_at), 1_709_287_200);
        assert_eq!(updated_unix(""), 0);
        assert!(parse_list_row(&json!({ "node": { "id": 1 } })).is_none());
        let u = parse_user(&json!({ "id": 7, "name": "me", "picture": "p" })).unwrap();
        assert_eq!(
            (u.id, u.name.as_str(), u.picture.as_deref()),
            (7, "me", Some("p"))
        );
    }

    #[test]
    fn update_form_only_sends_changes() {
        let f = update_form(&MalEntryInput {
            status: Some("completed".into()),
            score: Some(12),
            ..Default::default()
        });
        assert_eq!(
            f,
            vec![("status", "completed".to_owned()), ("score", "10".to_owned())]
        );
        assert!(update_form(&MalEntryInput::default()).is_empty());
    }

    #[test]
    fn status_codes() {
        assert!(Mal::check(200).is_ok());
        assert!(matches!(Mal::check(401), Err(ListError::Unauthorized)));
        assert!(matches!(Mal::check(429), Err(ListError::RateLimited)));
        assert!(matches!(Mal::check(500), Err(ListError::Http(500))));
    }
}
