//! MyAnimeList v2 API provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.mal`;
//! classic `lib/metadata/providers/mal.ts`). `https://api.myanimelist.net/v2` with `X-MAL-CLIENT-ID` =
//! `option_env!("TATAKAI_MAL_CLIENT_ID")` (the same public client id worker g's OAuth uses). Without it every call
//! answers `Unsupported` (the router skips MAL without a cool-down) and Settings > Metadata explains why
//! ([`Mal::has_client_id`]). Keyed by `MediaRef::mal_id` (AniList gives `idMal`): `media` maps one title, [`Mal::facts`]
//! returns the score / rank / popularity / synopsis tk-app uses to fill what AniList lacks on Details. MAL has no
//! episode list in v2 (`episodes` stays `Unsupported`) and no genre feeds (`feed` answers an empty page so the router
//! tries the next provider, as the classic did).
//!
//! [`ExternalFacts`] is shared with `tenrai.rs` (MAL data through Jikan v4) and `shikimori.rs`.

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use crate::error::ProviderError;
use crate::model::{FeedKind, FeedPage, Media, MediaKind, MediaRef};

pub const MAL_API_BASE: &str = "https://api.myanimelist.net/v2";
const DETAIL_FIELDS: &str = "id,title,main_picture,alternative_titles,synopsis,mean,rank,popularity,num_list_users,\
num_episodes,status,genres,start_season,media_type,nsfw,num_chapters";
const LIST_FIELDS: &str =
    "id,title,main_picture,alternative_titles,mean,num_episodes,status,genres,start_season,media_type,nsfw";
const PAGE_SIZE: u32 = 24;

/// What an external catalogue adds to a title on Details (`metadata.providers.*`): only the fields the provider
/// knows are set. `score` is 0..=100 (MAL / Shikimori give 0..=10), `rank` and `popularity_rank` are MAL rankings
/// (1 = best), `members` the list users.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExternalFacts {
    pub provider: &'static str,
    pub score: Option<u32>,
    pub rank: Option<u32>,
    pub popularity_rank: Option<u32>,
    pub members: Option<u32>,
    pub synopsis: Option<String>,
    /// Alternative titles (English, Japanese, synonyms, Russian on Shikimori), without duplicates.
    pub titles: Vec<String>,
    /// Absolute screenshot URLs.
    pub screenshots: Vec<String>,
    pub episodes: Option<u32>,
}

pub(crate) fn opt_str(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub(crate) fn opt_u32(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0)
}

/// `8.72` (number or string) -> `87`.
pub(crate) fn score_100(v: &Value) -> Option<u32> {
    let f = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))?;
    (f > 0.0).then(|| (f * 10.0).round().clamp(0.0, 100.0) as u32)
}

/// Push `t` unless empty or already present.
pub(crate) fn push_unique(out: &mut Vec<String>, t: Option<String>) {
    if let Some(t) = t.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
        && !out.contains(&t)
    {
        out.push(t);
    }
}

/// HTTP status -> provider error (shared by the three REST providers of this wave).
pub(crate) fn status_err(provider: &str, status: u16) -> ProviderError {
    if status == 429 {
        ProviderError::RateLimited {
            provider: provider.to_owned(),
        }
    } else {
        ProviderError::Http(status)
    }
}

/// MAL v2 status -> AniList spelling (`RELEASING`, ...), which the rest of the app shows.
fn status(s: &str) -> Option<String> {
    Some(
        match s {
            "currently_airing" | "currently_publishing" => "RELEASING",
            "finished_airing" | "finished" => "FINISHED",
            "not_yet_aired" | "not_yet_published" => "NOT_YET_RELEASED",
            "on_hiatus" => "HIATUS",
            "discontinued" => "CANCELLED",
            _ => return None,
        }
        .to_owned(),
    )
}

/// One MAL v2 node (`/anime/{id}` or a list `node`) -> [`Media`].
pub fn map_mal_node(n: &Value, kind: MediaKind) -> Option<Media> {
    let id = opt_u32(&n["id"])?;
    let mut titles = std::collections::BTreeMap::new();
    if let Some(t) = opt_str(&n["title"]) {
        titles.insert("romaji".to_owned(), t);
    }
    if let Some(t) = opt_str(&n["alternative_titles"]["en"]) {
        titles.insert("english".to_owned(), t);
    }
    if let Some(t) = opt_str(&n["alternative_titles"]["ja"]) {
        titles.insert("native".to_owned(), t);
    }
    let title = titles
        .get("english")
        .or_else(|| titles.get("romaji"))
        .cloned()
        .unwrap_or_default();
    let (season, year) = (
        opt_str(&n["start_season"]["season"]).map(|s| s.to_ascii_uppercase()),
        n["start_season"]["year"]
            .as_i64()
            .and_then(|y| i32::try_from(y).ok()),
    );
    Some(Media {
        media_ref: MediaRef {
            mal_id: Some(id),
            ..MediaRef::default()
        },
        kind,
        title,
        titles,
        cover: opt_str(&n["main_picture"]["large"]).or_else(|| opt_str(&n["main_picture"]["medium"])),
        banner: None,
        color: None,
        synopsis: opt_str(&n["synopsis"]),
        genres: n["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| opt_str(&g["name"])).collect())
            .unwrap_or_default(),
        episodes: opt_u32(&n["num_episodes"]).or_else(|| opt_u32(&n["num_chapters"])),
        status: n["status"].as_str().and_then(status),
        season,
        year,
        score: score_100(&n["mean"]),
        source_provider: "mal".into(),
    })
}

/// `/anime/{id}` -> [`ExternalFacts`].
pub fn map_mal_facts(n: &Value) -> ExternalFacts {
    let mut titles = Vec::new();
    push_unique(&mut titles, opt_str(&n["alternative_titles"]["en"]));
    push_unique(&mut titles, opt_str(&n["alternative_titles"]["ja"]));
    for s in n["alternative_titles"]["synonyms"]
        .as_array()
        .into_iter()
        .flatten()
    {
        push_unique(&mut titles, opt_str(s));
    }
    ExternalFacts {
        provider: "mal",
        score: score_100(&n["mean"]),
        rank: opt_u32(&n["rank"]),
        popularity_rank: opt_u32(&n["popularity"]),
        members: opt_u32(&n["num_list_users"]),
        synopsis: opt_str(&n["synopsis"]),
        titles,
        screenshots: Vec::new(),
        episodes: opt_u32(&n["num_episodes"]),
    }
}

pub struct Mal {
    client: reqwest::Client,
    client_id: Option<String>,
}

impl Mal {
    pub fn new(client: reqwest::Client) -> Self {
        let client_id = option_env!("TATAKAI_MAL_CLIENT_ID")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);
        Self { client, client_id }
    }

    /// Whether this build carries a MAL client id (Settings > Metadata explains it when not).
    pub fn has_client_id() -> bool {
        option_env!("TATAKAI_MAL_CLIENT_ID").is_some_and(|s| !s.trim().is_empty())
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Option<Value>, ProviderError> {
        let Some(id) = self.client_id.as_deref() else {
            return Err(ProviderError::Unsupported);
        };
        let resp = self
            .client
            .get(format!("{MAL_API_BASE}{path}"))
            .header("X-MAL-CLIENT-ID", id)
            .header("Accept", "application/json")
            .query(query)
            .send()
            .await?;
        let code = resp.status().as_u16();
        if code == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&code) {
            return Err(status_err("mal", code));
        }
        resp.json::<Value>()
            .await
            .map(Some)
            .map_err(|e| ProviderError::Parse(e.without_url().to_string()))
    }

    fn path(kind: MediaKind) -> &'static str {
        if kind == MediaKind::Manga {
            "manga"
        } else {
            "anime"
        }
    }

    /// Score, rank, popularity and synopsis of a MAL title (`None` = MAL does not know the id).
    pub async fn facts(&self, mal_id: u32) -> Result<Option<ExternalFacts>, ProviderError> {
        Ok(self
            .get(
                &format!("/anime/{mal_id}"),
                &[("fields", DETAIL_FIELDS.to_owned())],
            )
            .await?
            .as_ref()
            .map(map_mal_facts))
    }

    fn list(res: &Value, kind: MediaKind, page: u32) -> FeedPage {
        let items = res["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|row| &row["node"])
                    .filter(|n| n["nsfw"].as_str() != Some("black"))
                    .filter_map(|n| map_mal_node(n, kind))
                    .collect()
            })
            .unwrap_or_default();
        FeedPage {
            items,
            page,
            has_next: res["paging"]["next"].is_string(),
        }
    }
}

#[async_trait]
impl MetadataProvider for Mal {
    fn id(&self) -> &'static str {
        "mal"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime, MediaKind::Manga]
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        let page = page.max(1);
        let ranking = match (k, f) {
            (_, FeedKind::TopRated) => "all",
            (_, FeedKind::Popular) => "bypopularity",
            (MediaKind::Anime, FeedKind::Trending | FeedKind::Airing | FeedKind::Seasonal) => "airing",
            (MediaKind::Anime, FeedKind::Upcoming) => "upcoming",
            (MediaKind::Manga, FeedKind::Trending | FeedKind::Airing | FeedKind::Seasonal) => "bypopularity",
            // recently completed / manga upcoming: not offered by MAL v2; an empty page lets the next provider answer.
            _ => {
                return Ok(FeedPage {
                    items: Vec::new(),
                    page,
                    has_next: false,
                });
            }
        };
        let query = [
            ("ranking_type", ranking.to_owned()),
            ("limit", PAGE_SIZE.to_string()),
            ("offset", ((page - 1) * PAGE_SIZE).to_string()),
            ("fields", LIST_FIELDS.to_owned()),
        ];
        let res = self
            .get(&format!("/{}/ranking", Self::path(k)), &query)
            .await?
            .unwrap_or(Value::Null);
        Ok(Self::list(&res, k, page))
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let page = page.max(1);
        let q = q.trim();
        // MAL rejects queries shorter than 3 characters.
        if q.chars().count() < 3 {
            return Ok(FeedPage {
                items: Vec::new(),
                page,
                has_next: false,
            });
        }
        let query = [
            ("q", q.chars().take(64).collect::<String>()),
            ("limit", PAGE_SIZE.to_string()),
            ("offset", ((page - 1) * PAGE_SIZE).to_string()),
            ("fields", LIST_FIELDS.to_owned()),
        ];
        let res = self
            .get(&format!("/{}", Self::path(k)), &query)
            .await?
            .unwrap_or(Value::Null);
        Ok(Self::list(&res, k, page))
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        let Some(id) = r.mal_id.filter(|i| *i > 0) else {
            return Ok(None);
        };
        let fields = [("fields", DETAIL_FIELDS.to_owned())];
        let raw = self.get(&format!("/{}/{id}", Self::path(k)), &fields).await?;
        Ok(raw.as_ref().and_then(|n| map_mal_node(n, k)).map(|mut m| {
            m.media_ref.anilist_id = r.anilist_id;
            m.media_ref.kitsu_id = r.kitsu_id;
            m
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node() -> Value {
        json!({
            "id": 21, "title": "One Piece", "main_picture": { "medium": "m.jpg", "large": "l.jpg" },
            "alternative_titles": { "synonyms": ["OP", "One Piece"], "en": "One Piece", "ja": "ONE PIECE" },
            "synopsis": " Pirates. ", "mean": 8.72, "rank": 62, "popularity": 17, "num_list_users": 2744446,
            "num_episodes": 0, "status": "currently_airing", "genres": [{ "id": 1, "name": "Action" }],
            "start_season": { "year": 1999, "season": "fall" }, "media_type": "tv", "nsfw": "white"
        })
    }

    #[test]
    fn maps_a_node_to_media() {
        let m = map_mal_node(&node(), MediaKind::Anime).unwrap();
        assert_eq!(m.media_ref.mal_id, Some(21));
        assert_eq!(m.title, "One Piece");
        assert_eq!(m.titles.get("native").map(String::as_str), Some("ONE PIECE"));
        assert_eq!(m.cover.as_deref(), Some("l.jpg"));
        assert_eq!(m.status.as_deref(), Some("RELEASING"));
        assert_eq!((m.season.as_deref(), m.year), (Some("FALL"), Some(1999)));
        assert_eq!(m.score, Some(87));
        assert_eq!(m.episodes, None, "0 episodes = unknown");
        assert_eq!(m.genres, vec!["Action".to_owned()]);
        assert!(map_mal_node(&json!({ "title": "x" }), MediaKind::Anime).is_none());
    }

    #[test]
    fn maps_facts() {
        let f = map_mal_facts(&node());
        assert_eq!(
            (f.score, f.rank, f.popularity_rank, f.members),
            (Some(87), Some(62), Some(17), Some(2_744_446))
        );
        assert_eq!(f.synopsis.as_deref(), Some("Pirates."));
        assert_eq!(
            f.titles,
            vec!["One Piece".to_owned(), "ONE PIECE".to_owned(), "OP".to_owned()]
        );
    }

    #[test]
    fn helpers() {
        assert_eq!(score_100(&json!("8.72")), Some(87));
        assert_eq!(score_100(&json!(0.0)), None);
        assert_eq!(
            status_err("mal", 429),
            ProviderError::RateLimited {
                provider: "mal".into()
            }
        );
        assert_eq!(status_err("mal", 500), ProviderError::Http(500));
    }

    #[tokio::test]
    async fn without_a_client_id_nothing_is_asked() {
        let m = Mal {
            client: reqwest::Client::new(),
            client_id: None,
        };
        let r = MediaRef {
            mal_id: Some(1),
            ..MediaRef::default()
        };
        assert_eq!(
            m.media(MediaKind::Anime, &r).await,
            Err(ProviderError::Unsupported)
        );
        assert_eq!(m.facts(1).await, Err(ProviderError::Unsupported));
    }
}
