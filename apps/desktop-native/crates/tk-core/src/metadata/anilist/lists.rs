//! Authenticated AniList calls for the lists (wave A, worker **g**; docs/redesign/15 section 4): `Viewer`,
//! `MediaListCollection(userId, type: ANIME)`, `SaveMediaListEntry` / `DeleteMediaListEntry`,
//! `ToggleFavourite` (anime and characters), favourite state, the viewer's favourite anime and a batch MAL id ->
//! AniList id lookup. The bearer token comes from `crate::lists`; requests go through [`AniList::post_auth`] (the
//! shared `post` appends the card fragment to every query, which GraphQL rejects when a query does not use it).
//! Scores travel as `POINT_100` (`score(format: POINT_100)` to read, `scoreRaw` to write): AniList converts them to
//! the account's own scale (10, 100, 5 stars, 3 smileys).

use std::collections::HashMap;

use serde_json::{Value, json};

use super::{AniList, CARD_FRAGMENT};
use crate::error::ListError;
use crate::metadata::mapping::map_anilist_media;
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

const ENTRY_FIELDS: &str = "id status progress score(format: POINT_100) repeat notes updatedAt \
startedAt { year month day } completedAt { year month day } media { ...mediaCard }";

const VIEWER_QUERY: &str = "query { Viewer { id name avatar { large medium } siteUrl } }";

const FAVOURITE_MEDIA_QUERY: &str = "query ($id: Int) { Media(id: $id, type: ANIME) { id isFavourite } }";
const FAVOURITE_CHARACTER_QUERY: &str = "query ($id: Int) { Character(id: $id) { id isFavourite } }";
const CHARACTER_CARD_QUERY: &str =
    "query ($id: Int) { Character(id: $id) { id name { full userPreferred } image { large medium } } }";
const TOGGLE_ANIME: &str =
    "mutation ($animeId: Int) { ToggleFavourite(animeId: $animeId) { anime { pageInfo { total } } } }";
const TOGGLE_CHARACTER: &str = "mutation ($characterId: Int) { ToggleFavourite(characterId: $characterId) { characters { pageInfo { total } } } }";
const DELETE_ENTRY: &str = "mutation ($id: Int) { DeleteMediaListEntry(id: $id) { deleted } }";
const MAL_IDS_QUERY: &str = "query ($ids: [Int], $page: Int) { Page(page: $page, perPage: 50) { pageInfo { hasNextPage } media(idMal_in: $ids, type: ANIME) { id idMal } } }";

fn list_query() -> String {
    format!(
        "query ($userId: Int) {{ MediaListCollection(userId: $userId, type: ANIME) {{ lists {{ entries {{ {ENTRY_FIELDS} }} }} }} }} {CARD_FRAGMENT}"
    )
}

fn save_query() -> String {
    format!(
        "mutation ($mediaId: Int, $status: MediaListStatus, $progress: Int, $scoreRaw: Int, $notes: String) {{ \
SaveMediaListEntry(mediaId: $mediaId, status: $status, progress: $progress, scoreRaw: $scoreRaw, notes: $notes) {{ {ENTRY_FIELDS} }} }} {CARD_FRAGMENT}"
    )
}

fn favourites_query() -> String {
    format!(
        "query ($page: Int) {{ Viewer {{ favourites {{ anime(page: $page, perPage: 50) {{ pageInfo {{ hasNextPage }} nodes {{ ...mediaCard }} }} }} }} }} {CARD_FRAGMENT}"
    )
}

/// `{year, month, day}` -> `YYYY-MM-DD`; `None` without a year.
fn fuzzy(v: &Value) -> Option<String> {
    crate::lists::import::fuzzy_date(v)
}

/// One `MediaList` JSON object -> [`RemoteEntry`]. Pure: unit tested.
pub fn remote_entry(v: &Value) -> Option<RemoteEntry> {
    let id = v["id"].as_u64()?;
    if !v["media"].is_object() {
        return None;
    }
    Some(RemoteEntry {
        id,
        media: map_anilist_media(&v["media"]),
        status: v["status"].as_str().unwrap_or("PLANNING").to_owned(),
        progress: v["progress"]
            .as_u64()
            .map_or(0, |p| p.min(u64::from(u32::MAX)) as u32),
        score_100: v["score"]
            .as_f64()
            .filter(|s| *s > 0.0)
            .map(|s| s.min(100.0).round() as u8),
        started_at: fuzzy(&v["startedAt"]),
        completed_at: fuzzy(&v["completedAt"]),
        repeats: v["repeat"].as_u64().map_or(0, |r| r.min(10_000) as u32),
        notes: v["notes"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned),
        updated_at: v["updatedAt"].as_i64().unwrap_or(0),
    })
}

/// Every entry of a `MediaListCollection` answer, once (an entry is repeated in each custom list it belongs to).
pub fn collection_entries(body: &Value) -> Vec<RemoteEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for list in body["data"]["MediaListCollection"]["lists"]
        .as_array()
        .into_iter()
        .flatten()
    {
        for e in list["entries"].as_array().into_iter().flatten() {
            if let Some(r) = remote_entry(e)
                && seen.insert(r.id)
            {
                out.push(r);
            }
        }
    }
    out
}

/// GraphQL errors that mean "the token is no good".
fn is_auth_error(body: &Value) -> bool {
    body["errors"].as_array().is_some_and(|errs| {
        errs.iter().any(|e| {
            e["status"] == 401
                || e["message"].as_str().is_some_and(|m| {
                    let m = m.to_ascii_lowercase();
                    m.contains("invalid token") || m.contains("unauthorized") || m.contains("unauthenticated")
                })
        })
    })
}

/// `SaveMediaListEntry` variables: only the fields being changed.
pub fn save_vars(anilist_media_id: u32, input: &EntryInput) -> Value {
    let mut v = serde_json::Map::new();
    v.insert("mediaId".into(), anilist_media_id.into());
    if let Some(s) = &input.status {
        v.insert("status".into(), s.clone().into());
    }
    if let Some(p) = input.progress {
        v.insert("progress".into(), p.into());
    }
    if let Some(s) = input.score_100 {
        v.insert("scoreRaw".into(), s.min(100).into());
    }
    if let Some(n) = &input.notes {
        v.insert("notes".into(), n.clone().into());
    }
    Value::Object(v)
}

impl AniList {
    /// GraphQL POST with an optional bearer token; maps auth / rate-limit failures to [`ListError`].
    pub(crate) async fn post_auth(
        &self,
        token: Option<&str>,
        query: &str,
        variables: Value,
    ) -> Result<Value, ListError> {
        let mut req = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": variables }));
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        if status == 429 {
            return Err(ListError::RateLimited);
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| ListError::Parse(e.without_url().to_string()))?;
        if status == 401 || is_auth_error(&body) {
            return Err(ListError::Unauthorized);
        }
        if body["errors"]
            .as_array()
            .is_some_and(|e| e.iter().any(|x| x["status"] == 429))
        {
            return Err(ListError::RateLimited);
        }
        if !(200..300).contains(&status) {
            return Err(ListError::Http(status));
        }
        if body["data"].is_null() {
            let msg = body["errors"][0]["message"].as_str().unwrap_or("missing data");
            return Err(ListError::Parse(msg.to_owned()));
        }
        Ok(body)
    }

    pub async fn viewer(&self, token: &str) -> Result<Viewer, ListError> {
        let body = self.post_auth(Some(token), VIEWER_QUERY, json!({})).await?;
        let v = &body["data"]["Viewer"];
        let id = v["id"]
            .as_u64()
            .ok_or_else(|| ListError::Parse("Viewer.id".into()))?;
        Ok(Viewer {
            id,
            name: v["name"].as_str().unwrap_or_default().to_owned(),
            avatar: v["avatar"]["large"]
                .as_str()
                .or_else(|| v["avatar"]["medium"].as_str())
                .map(str::to_owned),
            site_url: v["siteUrl"].as_str().map(str::to_owned),
        })
    }

    /// Every anime entry of the viewer, all statuses.
    pub async fn media_list(&self, token: &str, user_id: u64) -> Result<Vec<RemoteEntry>, ListError> {
        let body = self
            .post_auth(Some(token), &list_query(), json!({ "userId": user_id }))
            .await?;
        Ok(collection_entries(&body))
    }

    /// Create / update; returns the saved entry.
    pub async fn save_entry(
        &self,
        token: &str,
        anilist_media_id: u32,
        input: &EntryInput,
    ) -> Result<RemoteEntry, ListError> {
        let body = self
            .post_auth(Some(token), &save_query(), save_vars(anilist_media_id, input))
            .await?;
        remote_entry(&body["data"]["SaveMediaListEntry"])
            .ok_or_else(|| ListError::Parse("SaveMediaListEntry".into()))
    }

    pub async fn delete_entry(&self, token: &str, entry_id: u64) -> Result<(), ListError> {
        match self
            .post_auth(Some(token), DELETE_ENTRY, json!({ "id": entry_id }))
            .await
        {
            Ok(_) => Ok(()),
            // Already gone on AniList: the goal is reached.
            Err(ListError::Http(404)) => Ok(()),
            Err(ListError::Parse(m)) if m.to_ascii_lowercase().contains("not found") => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// `ToggleFavourite(animeId:)`.
    pub async fn toggle_favourite(&self, token: &str, anilist_media_id: u32) -> Result<(), ListError> {
        self.post_auth(Some(token), TOGGLE_ANIME, json!({ "animeId": anilist_media_id }))
            .await
            .map(|_| ())
    }

    /// `ToggleFavourite(characterId:)`.
    pub async fn toggle_favourite_character(&self, token: &str, character_id: u32) -> Result<(), ListError> {
        self.post_auth(
            Some(token),
            TOGGLE_CHARACTER,
            json!({ "characterId": character_id }),
        )
        .await
        .map(|_| ())
    }

    /// Whether the viewer has this anime in their AniList favourites.
    pub async fn is_favourite(&self, token: &str, anilist_media_id: u32) -> Result<bool, ListError> {
        let body = self
            .post_auth(
                Some(token),
                FAVOURITE_MEDIA_QUERY,
                json!({ "id": anilist_media_id }),
            )
            .await?;
        Ok(body["data"]["Media"]["isFavourite"].as_bool().unwrap_or(false))
    }

    pub async fn is_favourite_character(&self, token: &str, character_id: u32) -> Result<bool, ListError> {
        let body = self
            .post_auth(
                Some(token),
                FAVOURITE_CHARACTER_QUERY,
                json!({ "id": character_id }),
            )
            .await?;
        Ok(body["data"]["Character"]["isFavourite"]
            .as_bool()
            .unwrap_or(false))
    }

    /// Set (not toggle) the favourite state of an anime (`character: false`) or a character.
    pub async fn set_favourite(
        &self,
        token: &str,
        id: u32,
        character: bool,
        favourite: bool,
    ) -> Result<(), ListError> {
        let now = if character {
            self.is_favourite_character(token, id).await?
        } else {
            self.is_favourite(token, id).await?
        };
        if now == favourite {
            return Ok(());
        }
        if character {
            self.toggle_favourite_character(token, id).await
        } else {
            self.toggle_favourite(token, id).await
        }
    }

    /// Name and portrait of a character (public, no token): the Favourites tab card.
    pub async fn character_card(
        &self,
        character_id: u32,
    ) -> Result<Option<(String, Option<String>)>, ListError> {
        let body = self
            .post_auth(None, CHARACTER_CARD_QUERY, json!({ "id": character_id }))
            .await?;
        let c = &body["data"]["Character"];
        if !c.is_object() {
            return Ok(None);
        }
        let name = c["name"]["userPreferred"]
            .as_str()
            .or_else(|| c["name"]["full"].as_str())
            .unwrap_or_default()
            .to_owned();
        let image = c["image"]["large"]
            .as_str()
            .or_else(|| c["image"]["medium"].as_str())
            .map(str::to_owned);
        Ok(Some((name, image)))
    }

    /// The viewer's favourite anime (all pages, at most 10).
    pub async fn favourite_anime(&self, token: &str) -> Result<Vec<Media>, ListError> {
        let mut out = Vec::new();
        for page in 1..=10_u32 {
            let body = self
                .post_auth(Some(token), &favourites_query(), json!({ "page": page }))
                .await?;
            let anime = &body["data"]["Viewer"]["favourites"]["anime"];
            out.extend(
                anime["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(map_anilist_media),
            );
            if anime["pageInfo"]["hasNextPage"] != true {
                break;
            }
        }
        Ok(out)
    }

    /// MAL ids -> AniList ids (public). Unknown ids are absent from the map.
    pub async fn ids_for_mal(&self, mal_ids: &[u32]) -> Result<HashMap<u32, u32>, ListError> {
        let mut out = HashMap::new();
        for chunk in mal_ids.chunks(50) {
            let body = self
                .post_auth(None, MAL_IDS_QUERY, json!({ "ids": chunk, "page": 1 }))
                .await?;
            for m in body["data"]["Page"]["media"].as_array().into_iter().flatten() {
                if let (Some(id), Some(mal)) = (m["id"].as_u64(), m["idMal"].as_u64())
                    && let (Ok(id), Ok(mal)) = (u32::try_from(id), u32::try_from(mal))
                {
                    out.insert(mal, id);
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, media: u64) -> Value {
        json!({ "id": id, "status": "CURRENT", "progress": 4, "score": 82.0, "repeat": 1, "notes": "",
                "updatedAt": 1700000000, "startedAt": { "year": 2024, "month": 1, "day": 7 }, "completedAt": { "year": null },
                "media": { "id": media, "idMal": 5, "type": "ANIME", "episodes": 12, "title": { "romaji": "R", "english": "E" },
                           "coverImage": { "large": "c" } } })
    }

    #[test]
    fn maps_entries_and_dedupes_custom_lists() {
        let body = json!({ "data": { "MediaListCollection": { "lists": [
            { "entries": [entry(1, 10), entry(2, 20)] }, { "entries": [entry(1, 10), { "id": 3 }] } ] } } });
        let all = collection_entries(&body);
        assert_eq!(all.len(), 2);
        let e = &all[0];
        assert_eq!(
            (e.id, e.media.media_ref.anilist_id, e.media.media_ref.mal_id),
            (1, Some(10), Some(5))
        );
        assert_eq!(
            (e.status.as_str(), e.progress, e.score_100, e.repeats),
            ("CURRENT", 4, Some(82), 1)
        );
        assert_eq!(
            (
                e.started_at.as_deref(),
                e.completed_at.as_deref(),
                e.notes.as_deref()
            ),
            (Some("2024-01-07"), None, None)
        );
        assert_eq!(e.media.title, "E");
        assert_eq!(e.updated_at, 1_700_000_000);
    }

    #[test]
    fn save_vars_only_carry_changes() {
        let v = save_vars(
            21,
            &EntryInput {
                status: Some("COMPLETED".into()),
                score_100: Some(140),
                ..Default::default()
            },
        );
        assert_eq!(
            v,
            json!({ "mediaId": 21, "status": "COMPLETED", "scoreRaw": 100 })
        );
        let v = save_vars(
            21,
            &EntryInput {
                progress: Some(3),
                notes: Some(String::new()),
                ..Default::default()
            },
        );
        assert_eq!(v, json!({ "mediaId": 21, "progress": 3, "notes": "" }));
    }

    #[test]
    fn auth_errors_are_recognised() {
        assert!(is_auth_error(
            &json!({ "errors": [{ "message": "Invalid token", "status": 400 }] })
        ));
        assert!(is_auth_error(
            &json!({ "errors": [{ "message": "x", "status": 401 }] })
        ));
        assert!(!is_auth_error(
            &json!({ "errors": [{ "message": "Not Found.", "status": 404 }] })
        ));
        assert!(!is_auth_error(&json!({ "data": {} })));
        assert!(list_query().contains("fragment mediaCard"));
        assert!(!VIEWER_QUERY.contains("mediaCard"));
    }
}
