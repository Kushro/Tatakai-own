//! AniList novel catalogue (wave B, worker **m**; checklist `novel.home`, `novel.search`, `novel.details`; classic
//! `core/content/novel-client.ts`: novels are AniList `MANGA` with `format: NOVEL`). A small client of its own so
//! worker m does not depend on the (parallel) manga queries of worker l: the same public GraphQL endpoint, the
//! same card mapping (`metadata::mapping::map_anilist_media`) plus the novel fields (chapters, volumes, authors,
//! country of origin, adult flag, synonyms for the extensions' title matching).

use serde_json::{Value, json};

use crate::error::ProviderError;
use crate::metadata::mapping::map_anilist_media;
use crate::model::Media;

const ENDPOINT: &str = "https://graphql.anilist.co";

const NOVEL_FRAGMENT: &str = "fragment novelCard on Media { id idMal type format status chapters volumes countryOfOrigin isAdult \
synonyms title { romaji english native } coverImage { extraLarge large medium color } bannerImage averageScore genres \
description(asHtml: false) startDate { year } }";

const PAGE_QUERY: &str = "query ($page: Int, $perPage: Int, $sort: [MediaSort], $search: String, $genre_in: [String], \
$status: MediaStatus, $countryOfOrigin: CountryCode, $isAdult: Boolean, $id_in: [Int]) { Page(page: $page, perPage: $perPage) { \
pageInfo { total currentPage hasNextPage } media(type: MANGA, format: NOVEL, sort: $sort, search: $search, genre_in: $genre_in, \
status: $status, countryOfOrigin: $countryOfOrigin, isAdult: $isAdult, id_in: $id_in) { ...novelCard } } }";

const DETAILS_QUERY: &str = "query ($id: Int) { Media(id: $id, type: MANGA) { ...novelCard \
staff(perPage: 8, sort: [RELEVANCE]) { edges { role node { name { full } } } } } }";

/// AniList genres offered as chips on the Novels home (the fixed AniList genre collection, novel-relevant subset).
pub const GENRES: [&str; 16] = [
    "Action",
    "Adventure",
    "Comedy",
    "Drama",
    "Ecchi",
    "Fantasy",
    "Horror",
    "Mahou Shoujo",
    "Mecha",
    "Mystery",
    "Psychological",
    "Romance",
    "Sci-Fi",
    "Slice of Life",
    "Sports",
    "Supernatural",
];

/// One novel: the shared `Media` card plus what the novel screens add.
#[derive(Debug, Clone, PartialEq)]
pub struct NovelInfo {
    pub media: Media,
    pub format: Option<String>,
    pub chapters: Option<u32>,
    pub volumes: Option<u32>,
    pub authors: Vec<String>,
    pub country: Option<String>,
    pub adult: bool,
    /// English, romaji, native and synonyms, best first (the extensions fuzzy-match them).
    pub titles: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NovelPage {
    pub items: Vec<NovelInfo>,
    pub page: u32,
    pub has_next: bool,
    pub total: Option<u32>,
}

/// Home rows (`novel.feed.<id>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NovelFeed {
    Trending,
    Popular,
    TopRated,
    Releasing,
}

impl NovelFeed {
    pub const ALL: [NovelFeed; 4] = [Self::Trending, Self::Popular, Self::TopRated, Self::Releasing];

    pub fn id(self) -> &'static str {
        match self {
            Self::Trending => "trending",
            Self::Popular => "popular",
            Self::TopRated => "top_rated",
            Self::Releasing => "releasing",
        }
    }
}

/// A filtered search (`novel.search`): genre, status, origin and order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NovelSearch {
    pub query: String,
    pub genres: Vec<String>,
    /// `releasing | finished | upcoming | cancelled | hiatus`.
    pub status: Option<String>,
    /// AniList `countryOfOrigin` (`JP`, `KR`, `CN`, `TW`).
    pub origin: Option<String>,
    /// `relevance | popularity | score | trending | newest | title`.
    pub sort: String,
    pub adult: bool,
    pub page: u32,
}

pub struct AniListNovels {
    client: reqwest::Client,
    endpoint: String,
}

fn u32_of(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// AniList `MediaStatus` of a UI status id.
pub fn status_var(status: &str) -> Option<&'static str> {
    match status {
        "releasing" => Some("RELEASING"),
        "finished" => Some("FINISHED"),
        "upcoming" => Some("NOT_YET_RELEASED"),
        "cancelled" => Some("CANCELLED"),
        "hiatus" => Some("HIATUS"),
        _ => None,
    }
}

/// AniList `MediaSort` of a UI sort id (`relevance` = text match with a query, popularity without).
pub fn sort_var(sort: &str, has_query: bool) -> &'static str {
    match sort {
        "popularity" => "POPULARITY_DESC",
        "score" => "SCORE_DESC",
        "trending" => "TRENDING_DESC",
        "newest" => "START_DATE_DESC",
        "title" => "TITLE_ROMAJI",
        _ if has_query => "SEARCH_MATCH",
        _ => "POPULARITY_DESC",
    }
}

/// Variables of a feed row.
pub fn feed_vars(feed: NovelFeed, page: u32) -> Value {
    let (sort, status) = match feed {
        NovelFeed::Trending => ("TRENDING_DESC", None),
        NovelFeed::Popular => ("POPULARITY_DESC", None),
        NovelFeed::TopRated => ("SCORE_DESC", None),
        NovelFeed::Releasing => ("POPULARITY_DESC", Some("RELEASING")),
    };
    let mut v = json!({ "page": page.max(1), "perPage": 20, "sort": [sort], "isAdult": false });
    if let Some(s) = status {
        v["status"] = s.into();
    }
    v
}

/// Variables of a search.
pub fn search_vars(q: &NovelSearch) -> Value {
    let query = q.query.trim();
    let mut v =
        json!({ "page": q.page.max(1), "perPage": 24, "sort": [sort_var(&q.sort, !query.is_empty())] });
    if !query.is_empty() {
        v["search"] = query.into();
    }
    if !q.genres.is_empty() {
        v["genre_in"] = json!(q.genres);
    }
    if let Some(s) = q.status.as_deref().and_then(status_var) {
        v["status"] = s.into();
    }
    if let Some(c) = q.origin.as_deref().filter(|c| super::ORIGINS.contains(c)) {
        v["countryOfOrigin"] = c.into();
    }
    if !q.adult {
        v["isAdult"] = false.into();
    }
    v
}

/// One `Media` object of the novel queries.
pub fn map_novel(m: &Value) -> NovelInfo {
    let mut media = map_anilist_media(m);
    media.year = m["startDate"]["year"]
        .as_i64()
        .and_then(|y| i32::try_from(y).ok());
    let mut titles: Vec<String> = Vec::new();
    for t in [
        &m["title"]["english"],
        &m["title"]["romaji"],
        &m["title"]["native"],
    ]
    .into_iter()
    .filter_map(Value::as_str)
    .chain(
        m["synonyms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    ) {
        let t = t.trim();
        if !t.is_empty() && !titles.iter().any(|x| x == t) {
            titles.push(t.to_owned());
        }
    }
    let authors = m["staff"]["edges"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|e| {
            let role = e["role"].as_str().unwrap_or_default().to_ascii_lowercase();
            role.contains("story")
                || role.contains("original")
                || role.contains("author")
                || role.contains("art")
        })
        .filter_map(|e| e["node"]["name"]["full"].as_str().map(str::to_owned))
        .fold(Vec::new(), |mut acc: Vec<String>, n| {
            if !acc.contains(&n) {
                acc.push(n);
            }
            acc
        });
    NovelInfo {
        media,
        format: m["format"].as_str().map(str::to_owned),
        chapters: u32_of(&m["chapters"]),
        volumes: u32_of(&m["volumes"]),
        authors,
        country: m["countryOfOrigin"].as_str().map(str::to_owned),
        adult: m["isAdult"].as_bool().unwrap_or(false),
        titles,
    }
}

/// `{ data: { Page } }` -> page. A body with errors and no data is an error.
pub fn map_page(doc: &Value) -> Result<NovelPage, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        let msg = doc["errors"][0]["message"]
            .as_str()
            .unwrap_or("missing data.Page");
        return Err(ProviderError::Parse(msg.to_owned()));
    }
    Ok(NovelPage {
        items: page["media"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|m| m["id"].as_u64().is_some_and(|id| id != 0))
            .map(map_novel)
            .collect(),
        page: u32_of(&page["pageInfo"]["currentPage"]).unwrap_or(1),
        has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        total: u32_of(&page["pageInfo"]["total"]),
    })
}

impl AniListNovels {
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            endpoint: ENDPOINT.to_owned(),
        }
    }

    /// Point at another GraphQL endpoint (tests).
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    async fn post(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": format!("{query} {NOVEL_FRAGMENT}"), "variables": variables }))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 429 {
            return Err(ProviderError::RateLimited {
                provider: "anilist".into(),
            });
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.without_url().to_string()))?;
        if body["errors"]
            .as_array()
            .is_some_and(|e| e.iter().any(|x| x["status"] == 429))
        {
            return Err(ProviderError::RateLimited {
                provider: "anilist".into(),
            });
        }
        if !(200..300).contains(&status) && status != 404 {
            return Err(ProviderError::Http(status));
        }
        Ok(body)
    }

    pub async fn feed(&self, feed: NovelFeed, page: u32) -> Result<NovelPage, ProviderError> {
        map_page(&self.post(PAGE_QUERY, feed_vars(feed, page)).await?)
    }

    pub async fn search(&self, q: &NovelSearch) -> Result<NovelPage, ProviderError> {
        map_page(&self.post(PAGE_QUERY, search_vars(q)).await?)
    }

    /// Cards of several ids at once ("Continue reading" rows whose title the progress table does not know).
    pub async fn by_ids(&self, ids: &[u64]) -> Result<Vec<NovelInfo>, ProviderError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let vars = json!({ "page": 1, "perPage": ids.len().min(50), "id_in": ids });
        Ok(map_page(&self.post(PAGE_QUERY, vars).await?)?.items)
    }

    /// One novel with its authors; `None` when AniList does not know the id.
    pub async fn details(&self, id: u64) -> Result<Option<NovelInfo>, ProviderError> {
        let body = self.post(DETAILS_QUERY, json!({ "id": id })).await?;
        let m = &body["data"]["Media"];
        Ok(m.is_object().then(|| map_novel(m)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_variables() {
        let q = NovelSearch {
            query: " overlord ".into(),
            genres: vec!["Fantasy".into()],
            status: Some("finished".into()),
            origin: Some("JP".into()),
            sort: "relevance".into(),
            adult: false,
            page: 0,
        };
        let v = search_vars(&q);
        assert_eq!(v["search"], "overlord");
        assert_eq!(v["sort"], json!(["SEARCH_MATCH"]));
        assert_eq!(v["genre_in"], json!(["Fantasy"]));
        assert_eq!(v["status"], "FINISHED");
        assert_eq!(v["countryOfOrigin"], "JP");
        assert_eq!(v["isAdult"], false);
        assert_eq!(v["page"], 1);
        let all = search_vars(&NovelSearch {
            origin: Some("XX".into()),
            adult: true,
            sort: "title".into(),
            ..NovelSearch::default()
        });
        assert!(all.get("countryOfOrigin").is_none());
        assert!(all.get("isAdult").is_none());
        assert_eq!(all["sort"], json!(["TITLE_ROMAJI"]));
        assert_eq!(sort_var("relevance", false), "POPULARITY_DESC");
        assert_eq!(feed_vars(NovelFeed::Releasing, 2)["status"], "RELEASING");
    }

    #[test]
    fn maps_a_novel() {
        let m = json!({
            "id": 85737, "idMal": 21479, "type": "MANGA", "format": "NOVEL", "status": "RELEASING", "chapters": null, "volumes": 16,
            "countryOfOrigin": "JP", "isAdult": false, "synonyms": ["Overlord LN", "Overlord"],
            "title": { "romaji": "Overlord", "english": "Overlord", "native": "オーバーロード" },
            "coverImage": { "extraLarge": "https://img/x.jpg", "color": "#e4a143" }, "bannerImage": null, "averageScore": 83,
            "genres": ["Action", "Fantasy"], "description": "Hello<br>world", "startDate": { "year": 2010 },
            "staff": { "edges": [
                { "role": "Story", "node": { "name": { "full": "Kugane Maruyama" } } },
                { "role": "Illustration", "node": { "name": { "full": "so-bin" } } },
                { "role": "Translator (English)", "node": { "name": { "full": "Someone" } } }
            ]}
        });
        let n = map_novel(&m);
        assert_eq!(n.media.media_ref.anilist_id, Some(85737));
        assert_eq!(n.media.year, Some(2010));
        assert_eq!(n.volumes, Some(16));
        assert_eq!(n.chapters, None);
        assert_eq!(n.titles, vec!["Overlord", "オーバーロード", "Overlord LN"]);
        assert_eq!(n.authors, vec!["Kugane Maruyama"]);
        assert_eq!(n.country.as_deref(), Some("JP"));
        let page = map_page(&json!({ "data": { "Page": { "pageInfo": { "total": 3, "currentPage": 1, "hasNextPage": true }, "media": [m, { "id": 0 }] } } })).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.has_next);
        assert_eq!(page.total, Some(3));
        assert!(map_page(&json!({ "errors": [{ "message": "boom" }] })).is_err());
    }
}
