//! MangaDex provider (wave B, worker **l**; checklist `manga.metadata.mangadex`; classic `core/content` MangaDex
//! client). `https://api.mangadex.org`: `manga?title=` search, `manga/{id}` with `includes[]=cover_art,author`,
//! `manga/{id}/feed` for chapter lists (language, group, pages) used by `crate::manga` as one more chapter
//! provider (`ext_id` [`BUILTIN_ID`]), `at-home/server/{chapter}` for the page URLs; mapping to `MediaRef`
//! through the `links.al` / `links.mal` attributes (AniList / MAL ids). Manga only.
//!
//! As a `MetadataProvider` it answers `search` and `feed` (popular / top rated / recently updated) for
//! `MediaKind::Manga`, which makes it the fallback of the manga search when AniList is down; `media` stays
//! `Unsupported` (a `MediaRef` carries no MangaDex id: tk-app asks [`MangaDex::manga`] with the `md:<uuid>` id).

use async_trait::async_trait;
use serde_json::Value;

use super::MetadataProvider;
use crate::error::ProviderError;
use crate::manga::{Chapter, Page, normalize_lang, parse_date, parse_number, parse_volume};
use crate::metadata::mapping::clean_synopsis;
use crate::model::{FeedKind, FeedPage, Media, MediaKind, MediaRef};

const BASE: &str = "https://api.mangadex.org";
const COVERS: &str = "https://uploads.mangadex.org/covers";
/// `ChapterRefVm::ext_id` of MangaDex chapters (not a valid extension id, so it never collides with one).
pub const BUILTIN_ID: &str = "@mangadex";
/// UI media id prefix of a MangaDex-only title (`md:<uuid>`).
pub const ID_PREFIX: &str = "md:";
const PAGE_LIMIT: u32 = 20;
/// Chapters per feed request (the API maximum) and the most requests per title.
const FEED_LIMIT: u32 = 500;
const MAX_FEED_PAGES: u32 = 6;

/// A MangaDex title with the fields the details page shows when AniList has no entry.
#[derive(Debug, Clone, PartialEq)]
pub struct MdManga {
    pub id: String,
    pub media: Media,
    pub alt_titles: Vec<String>,
    pub tags: Vec<String>,
    pub authors: Vec<String>,
    pub original_language: Option<String>,
    pub adult: bool,
    pub last_chapter: Option<f32>,
}

pub struct MangaDex {
    client: reqwest::Client,
    base: String,
}

impl MangaDex {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, base: BASE.to_owned() }
    }

    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    async fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ProviderError> {
        let url = url::Url::parse_with_params(&format!("{}{path}", self.base), params).map_err(|e| ProviderError::Parse(e.to_string()))?;
        let resp = self.client.get(url).header("Accept", "application/json").send().await?;
        match resp.status().as_u16() {
            404 => Ok(serde_json::json!({ "data": [] })),
            429 => Err(ProviderError::RateLimited { provider: "mangadex".into() }),
            s if !(200..300).contains(&s) => Err(ProviderError::Http(s)),
            _ => resp.json::<Value>().await.map_err(|e| ProviderError::Parse(e.without_url().to_string())),
        }
    }

    fn ratings(adult: bool) -> Vec<(&'static str, String)> {
        let mut v = vec![("contentRating[]", "safe".to_owned()), ("contentRating[]", "suggestive".to_owned())];
        if adult {
            v.push(("contentRating[]", "erotica".to_owned()));
            v.push(("contentRating[]", "pornographic".to_owned()));
        }
        v
    }

    /// Titles matching `query` (page 1-based).
    pub async fn search_titles(&self, query: &str, page: u32, adult: bool) -> Result<(Vec<MdManga>, bool), ProviderError> {
        let mut p = vec![
            ("title", query.trim().to_owned()),
            ("limit", PAGE_LIMIT.to_string()),
            ("offset", ((page.max(1) - 1) * PAGE_LIMIT).to_string()),
            ("includes[]", "cover_art".to_owned()),
            ("includes[]", "author".to_owned()),
            ("order[relevance]", "desc".to_owned()),
        ];
        p.extend(Self::ratings(adult));
        let body = self.get("/manga", &p).await?;
        Ok(map_manga_list(&body))
    }

    /// One title by MangaDex id.
    pub async fn manga(&self, id: &str) -> Result<Option<MdManga>, ProviderError> {
        let p = vec![("includes[]", "cover_art".to_owned()), ("includes[]", "author".to_owned()), ("includes[]", "artist".to_owned())];
        let body = self.get(&format!("/manga/{id}"), &p).await?;
        Ok(if body["data"].is_object() { map_manga(&body["data"]) } else { None })
    }

    /// The MangaDex id of a title known by AniList / MAL id and titles: the search hit whose `links.al` / `links.mal`
    /// match, else the first hit whose title equals one of `titles` (case and punctuation ignored).
    pub async fn find(&self, anilist_id: Option<u32>, mal_id: Option<u32>, titles: &[String]) -> Result<Option<String>, ProviderError> {
        let mut fallback = None;
        for t in titles.iter().filter(|t| !t.trim().is_empty()).take(3) {
            let mut p = vec![("title", t.trim().to_owned()), ("limit", "10".to_owned()), ("includes[]", "cover_art".to_owned())];
            p.extend(Self::ratings(true));
            let raw = self.get("/manga", &p).await?;
            if let Some(id) = match_links(&raw, anilist_id, mal_id) {
                return Ok(Some(id));
            }
            let (hits, _) = map_manga_list(&raw);
            if fallback.is_none() {
                fallback = hits.iter().find(|m| titles.iter().any(|x| same_title(x, &m.media.title) || m.alt_titles.iter().any(|a| same_title(x, a)))).map(|m| m.id.clone());
            }
        }
        Ok(fallback)
    }

    /// Every chapter of a title in `langs` (empty = every language), ascending.
    pub async fn chapters(&self, manga_id: &str, langs: &[String], adult: bool) -> Result<Vec<Chapter>, ProviderError> {
        let mut out = Vec::new();
        for page in 0..MAX_FEED_PAGES {
            let mut p = vec![
                ("limit", FEED_LIMIT.to_string()),
                ("offset", (page * FEED_LIMIT).to_string()),
                ("order[chapter]", "asc".to_owned()),
                ("includes[]", "scanlation_group".to_owned()),
            ];
            for l in langs {
                p.push(("translatedLanguage[]", l.clone()));
            }
            p.extend(Self::ratings(adult));
            let body = self.get(&format!("/manga/{manga_id}/feed"), &p).await?;
            let batch = map_feed(&body);
            let total = body["total"].as_u64().unwrap_or(0);
            out.extend(batch);
            if u64::from((page + 1) * FEED_LIMIT) >= total {
                break;
            }
        }
        Ok(out)
    }

    /// Page URLs of a chapter (`at-home/server/{id}`).
    pub async fn pages(&self, chapter_id: &str) -> Result<Vec<Page>, ProviderError> {
        let body = self.get(&format!("/at-home/server/{chapter_id}"), &[]).await?;
        Ok(map_at_home(&body))
    }
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned)
}

/// First value of a localized map, preferring `en`, then `ja-ro`, then any.
fn localized(v: &Value) -> Option<String> {
    let obj = v.as_object()?;
    for k in ["en", "ja-ro", "es", "es-la"] {
        if let Some(t) = obj.get(k).and_then(s) {
            return Some(t);
        }
    }
    obj.values().find_map(s)
}

fn fold(t: &str) -> String {
    t.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn same_title(a: &str, b: &str) -> bool {
    let (a, b) = (fold(a), fold(b));
    !a.is_empty() && a == b
}

/// The id of the hit whose links point at the AniList / MAL id.
fn match_links(body: &Value, anilist_id: Option<u32>, mal_id: Option<u32>) -> Option<String> {
    let hits = body["data"].as_array()?;
    let link = |m: &Value, key: &str| m["attributes"]["links"][key].as_str().and_then(|x| x.trim().parse::<u32>().ok());
    hits.iter()
        .find(|m| anilist_id.is_some() && link(m, "al") == anilist_id)
        .or_else(|| hits.iter().find(|m| mal_id.is_some() && link(m, "mal") == mal_id))
        .and_then(|m| s(&m["id"]))
}

/// A `manga` object (with `cover_art` / `author` relationships) -> [`MdManga`]; `None` without an id.
pub fn map_manga(m: &Value) -> Option<MdManga> {
    let id = s(&m["id"])?;
    let a = &m["attributes"];
    let title = localized(&a["title"]).or_else(|| a["altTitles"].as_array().and_then(|l| l.iter().find_map(localized)))?;
    let alt_titles: Vec<String> = a["altTitles"].as_array().into_iter().flatten().filter_map(|x| x.as_object()).flat_map(|o| o.values().filter_map(s)).collect();
    let rels = m["relationships"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let cover = rels
        .iter()
        .find(|r| r["type"] == "cover_art")
        .and_then(|r| s(&r["attributes"]["fileName"]))
        .map(|f| format!("{COVERS}/{id}/{f}.512.jpg"));
    let mut authors: Vec<String> = Vec::new();
    for r in rels.iter().filter(|r| r["type"] == "author" || r["type"] == "artist") {
        if let Some(n) = s(&r["attributes"]["name"])
            && !authors.contains(&n)
        {
            authors.push(n);
        }
    }
    let links = &a["links"];
    let num = |k: &str| links[k].as_str().and_then(|x| x.trim().parse::<u32>().ok()).filter(|n| *n > 0);
    let tags: Vec<String> = a["tags"].as_array().into_iter().flatten().filter_map(|t| localized(&t["attributes"]["name"])).collect();
    let genres = a["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["attributes"]["group"] == "genre")
        .filter_map(|t| localized(&t["attributes"]["name"]))
        .collect();
    let status = s(&a["status"]).map(|st| match st.as_str() {
        "ongoing" => "RELEASING".to_owned(),
        "completed" => "FINISHED".to_owned(),
        "hiatus" => "HIATUS".to_owned(),
        "cancelled" => "CANCELLED".to_owned(),
        other => other.to_ascii_uppercase(),
    });
    let rating = s(&a["contentRating"]).unwrap_or_default();
    let mut titles = std::collections::BTreeMap::new();
    titles.insert("english".to_owned(), title.clone());
    Some(MdManga {
        media: Media {
            media_ref: MediaRef { anilist_id: num("al"), mal_id: num("mal"), kitsu_id: None },
            kind: MediaKind::Manga,
            title,
            titles,
            cover,
            banner: None,
            color: None,
            synopsis: clean_synopsis(localized(&a["description"]).as_deref()),
            genres,
            episodes: None,
            status,
            season: None,
            year: a["year"].as_i64().and_then(|y| i32::try_from(y).ok()),
            score: None,
            source_provider: "mangadex".into(),
        },
        id,
        alt_titles,
        tags,
        authors,
        original_language: s(&a["originalLanguage"]),
        adult: matches!(rating.as_str(), "erotica" | "pornographic"),
        last_chapter: s(&a["lastChapter"]).map(|c| parse_number(&Value::String(c))).filter(|n| *n > 0.0),
    })
}

/// A `manga` list answer -> titles + has-next.
pub fn map_manga_list(body: &Value) -> (Vec<MdManga>, bool) {
    let items: Vec<MdManga> = body["data"].as_array().into_iter().flatten().filter_map(map_manga).collect();
    let total = body["total"].as_u64().unwrap_or(0);
    let offset = body["offset"].as_u64().unwrap_or(0);
    let has_next = offset + (items.len() as u64) < total;
    (items, has_next)
}

/// A `manga/{id}/feed` answer -> chapters (external-only chapters without pages are skipped).
pub fn map_feed(body: &Value) -> Vec<Chapter> {
    body["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let id = s(&c["id"])?;
            let a = &c["attributes"];
            let pages = a["pages"].as_u64().and_then(|p| u32::try_from(p).ok());
            if pages == Some(0) && s(&a["externalUrl"]).is_some() {
                return None;
            }
            let group = c["relationships"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|r| r["type"] == "scanlation_group")
                .and_then(|r| s(&r["attributes"]["name"]));
            Some(Chapter {
                ext_id: BUILTIN_ID.to_owned(),
                chapter_id: id,
                number: parse_number(&a["chapter"]),
                volume: parse_volume(&a["volume"]),
                title: s(&a["title"]),
                lang: s(&a["translatedLanguage"]).and_then(|l| normalize_lang(&l)),
                group,
                published_unix: parse_date(&a["publishAt"]).or_else(|| parse_date(&a["readableAt"])),
                pages: pages.filter(|p| *p > 0),
            })
        })
        .collect()
}

/// An `at-home/server` answer -> page URLs (full quality).
pub fn map_at_home(body: &Value) -> Vec<Page> {
    let (Some(base), Some(hash)) = (s(&body["baseUrl"]), s(&body["chapter"]["hash"])) else { return Vec::new() };
    body["chapter"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str())
        .enumerate()
        .map(|(i, f)| Page { index: i as u32, url: format!("{base}/data/{hash}/{f}"), headers: Vec::new(), width: None, height: None })
        .collect()
}

fn page_of(items: Vec<MdManga>, page: u32, has_next: bool) -> FeedPage {
    FeedPage { items: items.into_iter().map(|m| m.media).collect(), page, has_next }
}

#[async_trait]
impl MetadataProvider for MangaDex {
    fn id(&self) -> &'static str {
        "mangadex"
    }
    fn kinds(&self) -> &[MediaKind] {
        &[MediaKind::Manga]
    }

    async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        if k != MediaKind::Manga || q.trim().is_empty() {
            return Err(ProviderError::Unsupported);
        }
        let (items, has_next) = self.search_titles(q, page, false).await?;
        Ok(page_of(items, page.max(1), has_next))
    }

    async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        if k != MediaKind::Manga {
            return Err(ProviderError::Unsupported);
        }
        let order = match f {
            FeedKind::Trending | FeedKind::Popular => ("order[followedCount]", "desc"),
            FeedKind::TopRated => ("order[rating]", "desc"),
            FeedKind::Airing | FeedKind::Seasonal => ("order[latestUploadedChapter]", "desc"),
            FeedKind::Upcoming | FeedKind::RecentlyCompleted => return Err(ProviderError::Unsupported),
        };
        let mut p = vec![
            ("limit", PAGE_LIMIT.to_string()),
            ("offset", ((page.max(1) - 1) * PAGE_LIMIT).to_string()),
            ("includes[]", "cover_art".to_owned()),
            (order.0, order.1.to_owned()),
        ];
        p.extend(Self::ratings(false));
        let (items, has_next) = map_manga_list(&self.get("/manga", &p).await?);
        Ok(page_of(items, page.max(1), has_next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manga() -> Value {
        json!({
            "id": "uuid-1",
            "attributes": {
                "title": { "ja-ro": "Shingeki no Kyojin" },
                "altTitles": [ { "en": "Attack on Titan" }, { "es": "Ataque a los Titanes" } ],
                "description": { "en": "Humanity behind walls." },
                "links": { "al": "53390", "mal": "23390" },
                "status": "completed",
                "year": 2009,
                "contentRating": "suggestive",
                "originalLanguage": "ja",
                "lastChapter": "139",
                "tags": [ { "attributes": { "name": { "en": "Action" }, "group": "genre" } }, { "attributes": { "name": { "en": "Gore" }, "group": "content" } } ]
            },
            "relationships": [
                { "type": "cover_art", "attributes": { "fileName": "c.jpg" } },
                { "type": "author", "attributes": { "name": "Hajime Isayama" } },
                { "type": "artist", "attributes": { "name": "Hajime Isayama" } }
            ]
        })
    }

    #[test]
    fn maps_a_title_with_links_cover_and_status() {
        let m = map_manga(&manga()).unwrap();
        assert_eq!(m.id, "uuid-1");
        assert_eq!(m.media.title, "Shingeki no Kyojin");
        assert_eq!(m.media.media_ref.anilist_id, Some(53390));
        assert_eq!(m.media.media_ref.mal_id, Some(23390));
        assert_eq!(m.media.cover.as_deref(), Some("https://uploads.mangadex.org/covers/uuid-1/c.jpg.512.jpg"));
        assert_eq!(m.media.status.as_deref(), Some("FINISHED"));
        assert_eq!(m.media.genres, vec!["Action".to_owned()]);
        assert_eq!(m.tags.len(), 2);
        assert_eq!(m.authors, vec!["Hajime Isayama".to_owned()]);
        assert_eq!(m.last_chapter, Some(139.0));
        assert!(!m.adult);
        assert!(m.alt_titles.contains(&"Attack on Titan".to_owned()));
        assert!(map_manga(&json!({ "attributes": {} })).is_none());
    }

    #[test]
    fn links_match_beats_title_match() {
        let body = json!({ "data": [ { "id": "a", "attributes": { "links": { "al": "1" } } }, { "id": "b", "attributes": { "links": { "al": "53390" } } } ] });
        assert_eq!(match_links(&body, Some(53390), None).as_deref(), Some("b"));
        assert_eq!(match_links(&body, None, Some(5)), None);
        assert!(same_title("Attack on Titan!", "attack on titan"));
        assert!(!same_title("", ""));
    }

    #[test]
    fn feed_maps_chapters_and_skips_external_ones() {
        let body = json!({ "total": 3, "data": [
            { "id": "c1", "attributes": { "chapter": "1", "volume": "1", "title": "Intro", "translatedLanguage": "es-la", "pages": 40, "publishAt": "2020-01-01T00:00:00+00:00" },
              "relationships": [ { "type": "scanlation_group", "attributes": { "name": "Scans" } } ] },
            { "id": "c2", "attributes": { "chapter": "2", "translatedLanguage": "en", "pages": 0, "externalUrl": "https://x" } },
            { "id": "c3", "attributes": { "chapter": null, "translatedLanguage": "en", "pages": 10 } }
        ]});
        let ch = map_feed(&body);
        assert_eq!(ch.len(), 2);
        assert_eq!(ch[0].ext_id, BUILTIN_ID);
        assert_eq!(ch[0].lang.as_deref(), Some("es"));
        assert_eq!(ch[0].group.as_deref(), Some("Scans"));
        assert_eq!(ch[0].volume, Some(1));
        assert!(ch[0].published_unix.is_some());
        assert_eq!(ch[1].number, 0.0);
    }

    #[test]
    fn at_home_builds_page_urls() {
        let body = json!({ "baseUrl": "https://up.example", "chapter": { "hash": "h", "data": ["1.png", "2.png"] } });
        let pages = map_at_home(&body);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].url, "https://up.example/data/h/2.png");
        assert_eq!(pages[1].index, 1);
        assert!(map_at_home(&json!({})).is_empty());
    }

    #[test]
    fn list_has_next_from_total() {
        let body = json!({ "data": [manga()], "total": 5, "offset": 0 });
        let (items, more) = map_manga_list(&body);
        assert_eq!(items.len(), 1);
        assert!(more);
    }
}
