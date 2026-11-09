//! MangaUpdates provider (wave B, worker **l**; checklist `manga.metadata.mangaupdates`; classic `core/content`
//! MangaUpdates client). `https://api.mangaupdates.com/v1`: `series/search` (POST), `series/{id}`: publisher,
//! serialisation, publication status, cross-site rating. Manga only; it enriches the AniList row (never replaces
//! it), so the `MetadataProvider` impl answers `Unsupported` everywhere and tk-app calls [`MangaUpdates::lookup`].

use async_trait::async_trait;
use serde_json::{Value, json};

use super::MetadataProvider;
use crate::error::ProviderError;
use crate::model::MediaKind;

const BASE: &str = "https://api.mangaupdates.com/v1";

/// The facts the manga details page shows from MangaUpdates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MuSeries {
    pub id: u64,
    pub title: String,
    pub url: Option<String>,
    /// Publication status as MangaUpdates writes it (`"Complete (139 Chapters)"`, `"Ongoing"`).
    pub status: Option<String>,
    /// Original publishers.
    pub publishers: Vec<String>,
    /// Magazines it ran in (serialisation).
    pub serialization: Vec<String>,
    /// Licensed / English publishers.
    pub licensed: Vec<String>,
    pub completed: Option<bool>,
    /// Bayesian rating mapped to 0..=100.
    pub rating: Option<u32>,
    pub votes: Option<u32>,
    pub latest_chapter: Option<u32>,
    pub year: Option<i32>,
}

pub struct MangaUpdates {
    client: reqwest::Client,
    base: String,
}

impl MangaUpdates {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, base: BASE.to_owned() }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value, ProviderError> {
        let resp = req.header("Accept", "application/json").send().await?;
        match resp.status().as_u16() {
            404 => Ok(json!({})),
            429 => Err(ProviderError::RateLimited { provider: "mangaupdates".into() }),
            s if !(200..300).contains(&s) => Err(ProviderError::Http(s)),
            _ => resp.json::<Value>().await.map_err(|e| ProviderError::Parse(e.without_url().to_string())),
        }
    }

    /// Search hits: `(series id, title, year)`.
    pub async fn search(&self, title: &str) -> Result<Vec<(u64, String, Option<i32>)>, ProviderError> {
        let body = self.send(self.client.post(format!("{}/series/search", self.base)).json(&json!({ "search": title.trim(), "perpage": 10 }))).await?;
        Ok(map_search(&body))
    }

    pub async fn series(&self, id: u64) -> Result<Option<MuSeries>, ProviderError> {
        let body = self.send(self.client.get(format!("{}/series/{id}", self.base))).await?;
        Ok(map_series(&body))
    }

    /// The series of a title known by its names (first that has a hit) and start year: an exact (folded) title
    /// match first, then the same year, then the first hit.
    pub async fn lookup(&self, titles: &[String], year: Option<i32>) -> Result<Option<MuSeries>, ProviderError> {
        for t in titles.iter().filter(|t| !t.trim().is_empty()).take(2) {
            let hits = self.search(t).await?;
            if let Some(id) = pick(&hits, titles, year) {
                return self.series(id).await;
            }
        }
        Ok(None)
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned)
}

fn fold(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn year_of(v: &Value) -> Option<i32> {
    v.as_i64().and_then(|y| i32::try_from(y).ok()).or_else(|| v.as_str().and_then(|y| y.trim().get(..4)?.parse().ok()))
}

/// `series/search` answer -> hits.
pub fn map_search(body: &Value) -> Vec<(u64, String, Option<i32>)> {
    body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let rec = &r["record"];
            let id = rec["series_id"].as_u64()?;
            let title = s(&rec["title"]).or_else(|| s(&r["hit_title"]))?;
            Some((id, title, year_of(&rec["year"])))
        })
        .collect()
}

/// Best hit for `titles` / `year`.
pub fn pick(hits: &[(u64, String, Option<i32>)], titles: &[String], year: Option<i32>) -> Option<u64> {
    let folded: Vec<String> = titles.iter().map(|t| fold(t)).filter(|t| !t.is_empty()).collect();
    hits.iter()
        .find(|(_, t, _)| folded.contains(&fold(t)))
        .or_else(|| hits.iter().find(|(_, _, y)| year.is_some() && *y == year))
        .or_else(|| hits.first())
        .map(|(id, _, _)| *id)
}

/// `series/{id}` answer -> facts; `None` without an id.
pub fn map_series(v: &Value) -> Option<MuSeries> {
    let id = v["series_id"].as_u64()?;
    let names = |key: &str, field: &str, filter: &dyn Fn(&Value) -> bool| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in v[key].as_array().into_iter().flatten().filter(|p| filter(p)) {
            if let Some(n) = s(&p[field])
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
        out
    };
    let publishers = names("publishers", "publisher_name", &|p: &Value| p["type"].as_str().is_none_or(|t| t.eq_ignore_ascii_case("original")));
    let licensed = names("publishers", "publisher_name", &|p: &Value| p["type"].as_str().is_some_and(|t| !t.eq_ignore_ascii_case("original")));
    let serialization = names("publications", "publication_name", &|_: &Value| true);
    let rating = v["bayesian_rating"].as_f64().filter(|r| *r > 0.0).map(|r| (r * 10.0).round().clamp(0.0, 100.0) as u32);
    Some(MuSeries {
        id,
        title: s(&v["title"]).unwrap_or_default(),
        url: s(&v["url"]),
        status: s(&v["status"]).map(|st| st.replace('\n', " ")),
        publishers,
        serialization,
        licensed,
        completed: v["completed"].as_bool(),
        rating,
        votes: v["rating_votes"].as_u64().and_then(|n| u32::try_from(n).ok()),
        latest_chapter: v["latest_chapter"].as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0),
        year: year_of(&v["year"]),
    })
}

/// Enrichment only: every router call is `Unsupported` (the router skips it without a cool-down).
#[async_trait]
impl MetadataProvider for MangaUpdates {
    fn id(&self) -> &'static str {
        "mangaupdates"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Manga]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_hits_and_pick() {
        let body = json!({ "results": [
            { "record": { "series_id": 1, "title": "One Piece Party", "year": "2015" } },
            { "record": { "series_id": 2, "title": "One Piece", "year": "1997" } },
            { "record": { "title": "no id" } }
        ]});
        let hits = map_search(&body);
        assert_eq!(hits.len(), 2);
        assert_eq!(pick(&hits, &["ONE PIECE".into()], None), Some(2));
        assert_eq!(pick(&hits, &["x".into()], Some(2015)), Some(1));
        assert_eq!(pick(&hits, &["x".into()], None), Some(1));
        assert_eq!(pick(&[], &["x".into()], None), None);
    }

    #[test]
    fn series_facts() {
        let v = json!({
            "series_id": 55, "title": "One Piece", "url": "https://www.mangaupdates.com/series/x",
            "status": "1100+ Chapters\n(Ongoing)", "completed": false, "bayesian_rating": 9.05, "rating_votes": 3000,
            "latest_chapter": 1120, "year": "1997",
            "publishers": [ { "publisher_name": "Shueisha", "type": "Original" }, { "publisher_name": "VIZ Media", "type": "English" } ],
            "publications": [ { "publication_name": "Weekly Shonen Jump", "publisher_name": "Shueisha" } ]
        });
        let m = map_series(&v).unwrap();
        assert_eq!(m.publishers, vec!["Shueisha".to_owned()]);
        assert_eq!(m.licensed, vec!["VIZ Media".to_owned()]);
        assert_eq!(m.serialization, vec!["Weekly Shonen Jump".to_owned()]);
        assert_eq!(m.rating, Some(91));
        assert_eq!(m.status.as_deref(), Some("1100+ Chapters (Ongoing)"));
        assert_eq!(m.year, Some(1997));
        assert!(map_series(&json!({})).is_none());
    }
}
