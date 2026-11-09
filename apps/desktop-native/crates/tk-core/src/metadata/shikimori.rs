//! Shikimori provider (wave B, worker **b**; plan 14 section 0.1 #13; checklist `metadata.providers.shikimori`;
//! classic `lib/metadata/providers/shikimori.ts`). REST API at `https://shikimori.io/api` (the old `shikimori.one`
//! host answers `301` to it since 2026; checked alive on 2026-11-09). A Shikimori anime id **is** the MAL id, so it is
//! keyed by `MediaRef::mal_id`. `/animes/{id}`: alternative titles (`russian`, `english`, `japanese`, `synonyms`),
//! score, status, episodes; `/animes/{id}/screenshots`: screenshots (relative paths, made absolute here). A polite
//! `User-Agent` is required (the shared client sends `TatakaiNext/0.1`); limits 5 req/s, 90 req/min. Descriptions are
//! Russian BBCode, so `synopsis` is never taken from here. Shikimori is not a feed or search source in Next (AniList /
//! Kitsu cover those); `media` serves as a fallback for MAL-only ids.

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use super::mal::{ExternalFacts, opt_str, opt_u32, push_unique, score_100, status_err};
use crate::error::ProviderError;
use crate::model::{Media, MediaKind, MediaRef};

pub const SHIKIMORI_HOST: &str = "https://shikimori.io";
/// Screenshots kept per title.
pub const MAX_SCREENSHOTS: usize = 12;

/// Shikimori status -> AniList spelling.
fn status(s: &str) -> Option<String> {
    Some(
        match s {
            "ongoing" => "RELEASING",
            "released" => "FINISHED",
            "anons" => "NOT_YET_RELEASED",
            _ => return None,
        }
        .to_owned(),
    )
}

/// A path of the Shikimori host made absolute (`/system/...` -> `https://shikimori.io/system/...`).
pub fn absolute(path: &str) -> Option<String> {
    let p = path.trim();
    if p.is_empty() || p.contains("missing_") {
        return None;
    }
    if p.starts_with("https://") || p.starts_with("http://") {
        return Some(p.to_owned());
    }
    Some(format!("{SHIKIMORI_HOST}/{}", p.trim_start_matches('/')))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(opt_str).collect()).unwrap_or_default()
}

/// `/animes/{id}` -> [`Media`].
pub fn map_shikimori_anime(a: &Value) -> Option<Media> {
    let id = opt_u32(&a["id"])?;
    let mut titles = std::collections::BTreeMap::new();
    if let Some(t) = opt_str(&a["name"]) {
        titles.insert("romaji".to_owned(), t);
    }
    if let Some(t) = strings(&a["english"]).into_iter().next() {
        titles.insert("english".to_owned(), t);
    }
    if let Some(t) = strings(&a["japanese"]).into_iter().next() {
        titles.insert("native".to_owned(), t);
    }
    let title = titles.get("english").or_else(|| titles.get("romaji")).cloned().unwrap_or_default();
    let year = opt_str(&a["aired_on"]).and_then(|d| d.get(..4).and_then(|y| y.parse::<i32>().ok()));
    Some(Media {
        media_ref: MediaRef { mal_id: Some(id), ..MediaRef::default() },
        kind: MediaKind::Anime,
        title,
        titles,
        cover: a["image"]["original"].as_str().and_then(absolute),
        banner: None,
        color: None,
        synopsis: None,
        genres: a["genres"].as_array().map(|g| g.iter().filter_map(|x| opt_str(&x["name"])).collect()).unwrap_or_default(),
        episodes: opt_u32(&a["episodes"]),
        status: a["status"].as_str().and_then(status),
        season: None,
        year,
        score: score_100(&a["score"]),
        source_provider: "shikimori".into(),
    })
}

/// `/animes/{id}` + `/animes/{id}/screenshots` -> [`ExternalFacts`].
pub fn map_shikimori_facts(a: &Value, shots: &Value) -> ExternalFacts {
    let mut titles = Vec::new();
    for key in ["english", "japanese", "synonyms"] {
        for t in strings(&a[key]) {
            push_unique(&mut titles, Some(t));
        }
    }
    push_unique(&mut titles, opt_str(&a["russian"]));
    let screenshots = shots
        .as_array()
        .map(|s| s.iter().filter_map(|x| x["original"].as_str().and_then(absolute)).take(MAX_SCREENSHOTS).collect())
        .unwrap_or_default();
    ExternalFacts {
        provider: "shikimori",
        score: score_100(&a["score"]),
        titles,
        screenshots,
        episodes: opt_u32(&a["episodes"]).or_else(|| opt_u32(&a["episodes_aired"])),
        ..ExternalFacts::default()
    }
}

pub struct Shikimori {
    client: reqwest::Client,
    host: String,
}

impl Shikimori {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, host: SHIKIMORI_HOST.to_owned() }
    }

    async fn get(&self, path: &str) -> Result<Option<Value>, ProviderError> {
        let resp = self.client.get(format!("{}/api{path}", self.host)).header("Accept", "application/json").send().await?;
        let code = resp.status().as_u16();
        if code == 404 {
            return Ok(None);
        }
        if !(200..300).contains(&code) {
            return Err(status_err("shikimori", code));
        }
        resp.json::<Value>().await.map(Some).map_err(|e| ProviderError::Parse(e.without_url().to_string()))
    }

    /// Alternative titles, score and screenshots of a MAL id (`None` = unknown id). A failing screenshot call only
    /// leaves the screenshots empty.
    pub async fn facts(&self, mal_id: u32) -> Result<Option<ExternalFacts>, ProviderError> {
        let Some(anime) = self.get(&format!("/animes/{mal_id}")).await? else { return Ok(None) };
        let shots = match self.get(&format!("/animes/{mal_id}/screenshots")).await {
            Ok(Some(v)) => v,
            Ok(None) => Value::Null,
            Err(e) => {
                tracing::debug!(error = %e, mal_id, "shikimori screenshots failed");
                Value::Null
            }
        };
        Ok(Some(map_shikimori_facts(&anime, &shots)))
    }
}

#[async_trait]
impl MetadataProvider for Shikimori {
    fn id(&self) -> &'static str {
        "shikimori"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Anime]
    }

    async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        if k != MediaKind::Anime {
            return Err(ProviderError::Unsupported);
        }
        let Some(id) = r.mal_id.filter(|i| *i > 0) else { return Ok(None) };
        Ok(self.get(&format!("/animes/{id}")).await?.as_ref().and_then(map_shikimori_anime).map(|mut m| {
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

    fn anime() -> Value {
        json!({
            "id": 21, "name": "One Piece", "russian": "Ван-Пис", "image": { "original": "/system/animes/original/21.jpg?1" },
            "score": "8.72", "status": "ongoing", "episodes": 0, "episodes_aired": 1180, "aired_on": "1999-10-20",
            "english": ["One Piece"], "japanese": ["ONE PIECE"], "synonyms": ["Большой куш", "One Piece"],
            "genres": [{ "name": "Action", "russian": "Экшен" }]
        })
    }

    #[test]
    fn maps_anime_and_facts() {
        let m = map_shikimori_anime(&anime()).unwrap();
        assert_eq!(m.media_ref.mal_id, Some(21));
        assert_eq!(m.cover.as_deref(), Some("https://shikimori.io/system/animes/original/21.jpg?1"));
        assert_eq!((m.year, m.score, m.status.as_deref()), (Some(1999), Some(87), Some("RELEASING")));
        assert_eq!(m.synopsis, None);
        let shots = json!([{ "original": "/system/screenshots/original/a.jpg" }, { "original": "" }]);
        let f = map_shikimori_facts(&anime(), &shots);
        assert_eq!(f.titles, vec!["One Piece".to_owned(), "ONE PIECE".to_owned(), "Большой куш".to_owned(), "Ван-Пис".to_owned()]);
        assert_eq!(f.screenshots, vec!["https://shikimori.io/system/screenshots/original/a.jpg".to_owned()]);
        assert_eq!((f.score, f.episodes, f.provider), (Some(87), Some(1180), "shikimori"));
    }

    #[test]
    fn absolute_paths() {
        assert_eq!(absolute("https://x/y.jpg").as_deref(), Some("https://x/y.jpg"));
        assert_eq!(absolute("/assets/globals/missing_original.jpg"), None);
        assert_eq!(absolute("  "), None);
    }
}
