//! AniList filtered search, studio pages and character pages (wave A, worker **a**; docs/redesign/15 section 4).
//!
//! One `Page(page, perPage) { pageInfo media(type, search, genre_in, tag_in, format_in, status_in, season,
//! seasonYear, startDate_greater/lesser, episodes_greater/lesser, averageScore_greater/lesser, sort, isAdult) }`
//! query built from [`SearchVars`] (pure: [`AniList::search_vars`]); `Studio(search:) { media(page, sort) }` for
//! studios; `Character(id:)` with its anime appearances and their voice actors for character pages. Besides the
//! three contract entry points this file offers the extras the Search screens need: the page with its `total`
//! and adult flags ([`AniList::search_filtered_page`]), trending windows (`Page.mediaTrends`), the tag collection
//! (`MediaTagCollection`), a character search and a batch lookup by id (trace.moe results).
//!
//! AniList's `_greater` / `_lesser` arguments are exclusive, so inclusive UI ranges are widened by one unit here
//! (`year_from = 2010` -> `startDate_greater: 20100000`, `episodes_max = 13` -> `episodes_lesser: 14`).

use std::collections::HashSet;

use serde_json::{Value, json};

use super::AniList;
use crate::error::ProviderError;
use crate::metadata::mapping::{clean_synopsis, map_anilist_media, map_anilist_page};
use crate::model::{FeedPage, Media, MediaKind};

/// Sort orders accepted by `search_filtered` (mirror of the UI `SearchSortVm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchSort {
    #[default]
    Relevance,
    Popularity,
    Score,
    Trending,
    Newest,
    Title,
}

impl SearchSort {
    /// AniList `MediaSort` for this order. `Relevance` is text match with a query, popularity without one.
    pub fn anilist(self, has_query: bool) -> &'static str {
        match self {
            Self::Relevance if has_query => "SEARCH_MATCH",
            Self::Relevance | Self::Popularity => "POPULARITY_DESC",
            Self::Score => "SCORE_DESC",
            Self::Trending => "TRENDING_DESC",
            Self::Newest => "START_DATE_DESC",
            Self::Title => "TITLE_ROMAJI",
        }
    }
}

/// Variables of the filtered search; `None` / empty = not filtered. Strings use AniList's own enum spellings
/// (`format` `TV | MOVIE | ...`, `status` `RELEASING | FINISHED | NOT_YET_RELEASED | CANCELLED | HIATUS`, `season`
/// `WINTER | SPRING | SUMMER | FALL`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchVars {
    pub kind: Option<MediaKind>,
    pub query: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub episodes_min: Option<u32>,
    pub episodes_max: Option<u32>,
    pub score_min: Option<u32>,
    pub score_max: Option<u32>,
    pub sort: SearchSort,
    pub adult: bool,
    pub page: u32,
    pub per_page: u32,
}

impl SearchVars {
    /// True when only the text (and the default order) is set: the plain search any provider can answer.
    pub fn is_plain(&self) -> bool {
        self.genres.is_empty()
            && self.tags.is_empty()
            && self.format.is_none()
            && self.status.is_none()
            && self.season.is_none()
            && self.season_year.is_none()
            && self.year_from.is_none()
            && self.year_to.is_none()
            && self.episodes_min.is_none()
            && self.episodes_max.is_none()
            && self.score_min.is_none()
            && self.score_max.is_none()
            && self.sort == SearchSort::Relevance
            && !self.adult
    }
}

/// A character page as AniList returns it (mapped to the UI `CharacterVm` by tk-app).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Character {
    pub id: u32,
    pub name: String,
    pub native_name: Option<String>,
    pub alternative_names: Vec<String>,
    pub image: Option<String>,
    pub description: Option<String>,
    pub gender: Option<String>,
    pub age: Option<String>,
    /// `(year, month, day)` as AniList's fuzzy date (any part may be missing).
    pub birthday: (Option<i32>, Option<u32>, Option<u32>),
    pub blood_type: Option<String>,
    pub favourites: Option<u32>,
    pub appearances: Vec<CharacterAppearance>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CharacterAppearance {
    pub media: crate::model::Media,
    /// `MAIN | SUPPORTING | BACKGROUND`
    pub role: String,
    pub voice_actors: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
    /// Voice actor language (`Japanese`) or staff role (`Director`).
    pub role: Option<String>,
}

/// A page of titles with what [`FeedPage`] does not carry: AniList's `pageInfo.total` and which entries are 18+.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResultPage {
    pub page: FeedPage,
    pub total: Option<u32>,
    /// AniList ids of the entries with `isAdult: true`.
    pub adult_ids: HashSet<u32>,
}

impl Default for SearchResultPage {
    /// An empty first page (`FeedPage` itself has no `Default`).
    fn default() -> Self {
        Self { page: FeedPage { items: Vec::new(), page: 1, has_next: false }, total: None, adult_ids: HashSet::new() }
    }
}

/// One entry of AniList's `MediaTagCollection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTag {
    pub name: String,
    pub category: String,
    /// `isGeneralSpoiler`: the tag itself reveals the plot.
    pub spoiler: bool,
    pub adult: bool,
}

/// One hit of the character search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterHit {
    pub id: u32,
    pub name: String,
    pub native_name: Option<String>,
    pub image: Option<String>,
    pub favourites: Option<u32>,
}

/// A page of [`CharacterHit`]s.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CharacterPage {
    pub items: Vec<CharacterHit>,
    pub page: u32,
    pub has_next: bool,
    pub total: Option<u32>,
}

/// Voice actor languages listed on character pages (one aliased `voiceActors(language:)` per language).
pub const VOICE_LANGUAGES: [(&str, &str); 3] = [("ja", "JAPANESE"), ("en", "ENGLISH"), ("es", "SPANISH")];

const SEARCH_QUERY: &str = "query ($page: Int, $perPage: Int, $type: MediaType, $search: String, $genre_in: [String], \
$tag_in: [String], $format_in: [MediaFormat], $status_in: [MediaStatus], $season: MediaSeason, $seasonYear: Int, \
$startDate_greater: FuzzyDateInt, $startDate_lesser: FuzzyDateInt, $episodes_greater: Int, $episodes_lesser: Int, \
$averageScore_greater: Int, $averageScore_lesser: Int, $sort: [MediaSort], $isAdult: Boolean) { \
Page(page: $page, perPage: $perPage) { pageInfo { total currentPage lastPage hasNextPage perPage } \
media(type: $type, search: $search, genre_in: $genre_in, tag_in: $tag_in, format_in: $format_in, status_in: $status_in, \
season: $season, seasonYear: $seasonYear, startDate_greater: $startDate_greater, startDate_lesser: $startDate_lesser, \
episodes_greater: $episodes_greater, episodes_lesser: $episodes_lesser, averageScore_greater: $averageScore_greater, \
averageScore_lesser: $averageScore_lesser, sort: $sort, isAdult: $isAdult) { ...mediaCard isAdult } } }";

const STUDIO_QUERY: &str = "query ($search: String, $page: Int, $perPage: Int) { Studio(search: $search) { id name \
media(page: $page, perPage: $perPage, sort: [POPULARITY_DESC]) { pageInfo { total currentPage lastPage hasNextPage perPage } \
nodes { ...mediaCard isAdult } } } }";

const TRENDS_QUERY: &str = "query ($page: Int, $perPage: Int, $date_greater: Int) { Page(page: $page, perPage: $perPage) { \
pageInfo { total currentPage lastPage hasNextPage perPage } mediaTrends(date_greater: $date_greater, sort: [TRENDING_DESC]) { \
mediaId trending media { ...mediaCard isAdult } } } }";

const IDS_QUERY: &str = "query ($ids: [Int], $perPage: Int) { Page(page: 1, perPage: $perPage) { \
pageInfo { total currentPage lastPage hasNextPage perPage } media(id_in: $ids) { ...mediaCard isAdult } } }";

const TAGS_QUERY: &str = "query { MediaTagCollection { name category isGeneralSpoiler isAdult } }";

const CHARACTER_SEARCH_QUERY: &str = "query ($search: String, $page: Int, $perPage: Int) { Page(page: $page, perPage: $perPage) { \
pageInfo { total currentPage lastPage hasNextPage perPage } characters(search: $search, sort: [SEARCH_MATCH, FAVOURITES_DESC]) { \
id name { full native } image { large medium } favourites } } }";

fn character_query() -> String {
    let actors: String = VOICE_LANGUAGES
        .iter()
        .map(|(alias, lang)| {
            format!("{alias}: voiceActors(language: {lang}, sort: [RELEVANCE, ID]) {{ id name {{ full }} image {{ medium }} languageV2 }} ")
        })
        .collect();
    format!(
        "query ($id: Int) {{ Character(id: $id) {{ id name {{ full native alternative }} image {{ large }} \
description(asHtml: false) gender age dateOfBirth {{ year month day }} bloodType favourites \
media(page: 1, perPage: 25, sort: [POPULARITY_DESC], type: ANIME) {{ edges {{ characterRole {actors}node {{ ...mediaCard }} }} }} }} }}"
    )
}

fn opt_u32(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The GraphQL error message of a body without data (`None` when there is data).
fn graphql_error(doc: &Value) -> Option<String> {
    doc["errors"][0]["message"].as_str().map(str::to_owned)
}

/// Maps a `Page { pageInfo media[] }` document, keeping `total` and the `isAdult` flags.
pub fn map_search_page(doc: &Value) -> Result<SearchResultPage, ProviderError> {
    let page = map_anilist_page(doc)?;
    let raw = &doc["data"]["Page"];
    Ok(SearchResultPage { page, total: opt_u32(&raw["pageInfo"]["total"]), adult_ids: adult_ids(&raw["media"]) })
}

fn adult_ids(list: &Value) -> HashSet<u32> {
    list.as_array()
        .map(|a| a.iter().filter(|m| m["isAdult"].as_bool() == Some(true)).filter_map(|m| opt_u32(&m["id"])).collect())
        .unwrap_or_default()
}

fn media_list(list: &Value) -> Vec<Media> {
    list.as_array()
        .map(|a| a.iter().filter(|m| opt_u32(&m["id"]).is_some_and(|id| id != 0)).map(map_anilist_media).collect())
        .unwrap_or_default()
}

/// Maps `Studio { media { pageInfo nodes[] } }`. An unknown studio is an empty first page.
pub fn map_studio_page(doc: &Value, adult: bool) -> Result<SearchResultPage, ProviderError> {
    let studio = &doc["data"]["Studio"];
    if studio.is_null() {
        if let Some(msg) = graphql_error(doc)
            && !msg.to_ascii_lowercase().contains("not found")
        {
            return Err(ProviderError::Parse(msg));
        }
        return Ok(SearchResultPage::default());
    }
    let conn = &studio["media"];
    let flagged = adult_ids(&conn["nodes"]);
    let items: Vec<Media> = media_list(&conn["nodes"])
        .into_iter()
        .filter(|m| m.kind == MediaKind::Anime)
        .filter(|m| adult || m.media_ref.anilist_id.is_none_or(|id| !flagged.contains(&id)))
        .collect();
    Ok(SearchResultPage {
        page: FeedPage {
            items,
            page: opt_u32(&conn["pageInfo"]["currentPage"]).unwrap_or(1),
            has_next: conn["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        },
        total: opt_u32(&conn["pageInfo"]["total"]),
        adult_ids: flagged,
    })
}

/// Maps `Page { mediaTrends[] }`: one row per title and day, so titles are deduplicated (first = highest trend),
/// manga dropped and 18+ dropped unless `adult`.
pub fn map_trends_page(doc: &Value, adult: bool) -> Result<SearchResultPage, ProviderError> {
    let raw = &doc["data"]["Page"];
    if raw.is_null() {
        return Err(ProviderError::Parse(graphql_error(doc).unwrap_or_else(|| "missing data.Page".into())));
    }
    let mut seen = HashSet::new();
    let mut flagged = HashSet::new();
    let mut items = Vec::new();
    for row in raw["mediaTrends"].as_array().into_iter().flatten() {
        let m = &row["media"];
        let Some(id) = opt_u32(&m["id"]).filter(|id| *id != 0) else { continue };
        if m["type"] != "ANIME" || !seen.insert(id) {
            continue;
        }
        if m["isAdult"].as_bool() == Some(true) {
            if !adult {
                continue;
            }
            flagged.insert(id);
        }
        items.push(map_anilist_media(m));
    }
    Ok(SearchResultPage {
        page: FeedPage {
            items,
            page: opt_u32(&raw["pageInfo"]["currentPage"]).unwrap_or(1),
            has_next: raw["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        },
        total: None,
        adult_ids: flagged,
    })
}

/// Maps `MediaTagCollection`, sorted by category then name.
pub fn map_tags(doc: &Value) -> Result<Vec<MediaTag>, ProviderError> {
    let Some(list) = doc["data"]["MediaTagCollection"].as_array() else {
        return Err(ProviderError::Parse(graphql_error(doc).unwrap_or_else(|| "missing MediaTagCollection".into())));
    };
    let mut tags: Vec<MediaTag> = list
        .iter()
        .filter_map(|t| {
            Some(MediaTag {
                name: opt_str(&t["name"])?,
                category: opt_str(&t["category"]).unwrap_or_default(),
                spoiler: t["isGeneralSpoiler"].as_bool().unwrap_or(false),
                adult: t["isAdult"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    tags.sort_by(|a, b| a.category.cmp(&b.category).then_with(|| a.name.cmp(&b.name)));
    Ok(tags)
}

/// Maps `Page { characters[] }`.
pub fn map_character_page(doc: &Value) -> Result<CharacterPage, ProviderError> {
    let raw = &doc["data"]["Page"];
    if raw.is_null() {
        return Err(ProviderError::Parse(graphql_error(doc).unwrap_or_else(|| "missing data.Page".into())));
    }
    let items = raw["characters"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| {
                    Some(CharacterHit {
                        id: opt_u32(&c["id"]).filter(|id| *id != 0)?,
                        name: opt_str(&c["name"]["full"]).or_else(|| opt_str(&c["name"]["native"]))?,
                        native_name: opt_str(&c["name"]["native"]),
                        image: opt_str(&c["image"]["large"]).or_else(|| opt_str(&c["image"]["medium"])),
                        favourites: opt_u32(&c["favourites"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(CharacterPage {
        items,
        page: opt_u32(&raw["pageInfo"]["currentPage"]).unwrap_or(1),
        has_next: raw["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        total: opt_u32(&raw["pageInfo"]["total"]),
    })
}

/// AniList character descriptions are markdown with `~!spoiler!~` blocks: spoilers are dropped, `__bold__` /
/// `**bold**` / `_italic_` markers removed, `[text](url)` links reduced to their text, then cleaned like synopses.
pub fn clean_character_description(text: Option<&str>) -> Option<String> {
    let text = text?;
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("~!") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("!~") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    // [text](url) -> text
    let mut linked = String::with_capacity(out.len());
    let mut s = out.as_str();
    while let Some(open) = s.find('[') {
        let Some(close_rel) = s[open..].find("](") else { break };
        let close = open + close_rel;
        let Some(end_rel) = s[close + 2..].find(')') else { break };
        linked.push_str(&s[..open]);
        linked.push_str(&s[open + 1..close]);
        s = &s[close + 2 + end_rel + 1..];
    }
    linked.push_str(s);
    let stripped = linked.replace("__", "").replace("**", "");
    clean_synopsis(Some(&stripped))
}

fn person(v: &Value) -> Option<Person> {
    Some(Person {
        id: opt_u32(&v["id"]).filter(|id| *id != 0)?,
        name: opt_str(&v["name"]["full"])?,
        image: opt_str(&v["image"]["medium"]),
        role: opt_str(&v["languageV2"]),
    })
}

/// Maps `Character(id:)`; `Ok(None)` when AniList does not know it.
pub fn map_character(doc: &Value) -> Result<Option<Character>, ProviderError> {
    let c = &doc["data"]["Character"];
    if !c.is_object() {
        return match graphql_error(doc) {
            Some(msg) if !msg.to_ascii_lowercase().contains("not found") => Err(ProviderError::Parse(msg)),
            _ => Ok(None),
        };
    }
    let Some(id) = opt_u32(&c["id"]) else { return Ok(None) };
    let name = opt_str(&c["name"]["full"]).or_else(|| opt_str(&c["name"]["native"])).unwrap_or_default();
    let alternative_names = c["name"]["alternative"]
        .as_array()
        .map(|a| a.iter().filter_map(opt_str).filter(|n| *n != name).collect())
        .unwrap_or_default();
    let mut appearances = Vec::new();
    for edge in c["media"]["edges"].as_array().into_iter().flatten() {
        let node = &edge["node"];
        if opt_u32(&node["id"]).is_none_or(|id| id == 0) {
            continue;
        }
        let mut voice_actors: Vec<Person> = Vec::new();
        for (alias, _) in VOICE_LANGUAGES {
            for p in edge[alias].as_array().into_iter().flatten().filter_map(person) {
                if !voice_actors.iter().any(|x| x.id == p.id) {
                    voice_actors.push(p);
                }
            }
        }
        appearances.push(CharacterAppearance {
            media: map_anilist_media(node),
            role: opt_str(&edge["characterRole"]).unwrap_or_else(|| "BACKGROUND".into()),
            voice_actors,
        });
    }
    let dob = &c["dateOfBirth"];
    let year = dob["year"].as_i64().and_then(|y| i32::try_from(y).ok());
    Ok(Some(Character {
        id,
        name,
        native_name: opt_str(&c["name"]["native"]),
        alternative_names,
        image: opt_str(&c["image"]["large"]),
        description: clean_character_description(c["description"].as_str()),
        gender: opt_str(&c["gender"]),
        age: opt_str(&c["age"]),
        birthday: (year, opt_u32(&dob["month"]), opt_u32(&dob["day"])),
        blood_type: opt_str(&c["bloodType"]),
        favourites: opt_u32(&c["favourites"]),
        appearances,
    }))
}

impl AniList {
    /// GraphQL variables for `SearchVars` (pure, testable).
    pub fn search_vars(v: &SearchVars) -> Value {
        let mut m = serde_json::Map::new();
        m.insert("page".into(), v.page.max(1).into());
        m.insert("perPage".into(), v.per_page.clamp(1, 50).into());
        if let Some(k) = v.kind {
            m.insert("type".into(), k.as_str().into());
        }
        let query = v.query.split_whitespace().collect::<Vec<_>>().join(" ");
        if !query.is_empty() {
            m.insert("search".into(), query.clone().into());
        }
        let clean = |list: &[String]| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for s in list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                if !out.iter().any(|x| x == s) {
                    out.push(s.to_owned());
                }
            }
            out
        };
        let genres = clean(&v.genres);
        if !genres.is_empty() {
            m.insert("genre_in".into(), json!(genres));
        }
        let tags = clean(&v.tags);
        if !tags.is_empty() {
            m.insert("tag_in".into(), json!(tags));
        }
        if let Some(f) = v.format.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            m.insert("format_in".into(), json!([f.to_ascii_uppercase()]));
        }
        if let Some(s) = v.status.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            m.insert("status_in".into(), json!([s.to_ascii_uppercase()]));
        }
        if let Some(s) = v.season.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            m.insert("season".into(), s.to_ascii_uppercase().into());
        }
        let ranged = v.year_from.is_some() || v.year_to.is_some();
        match (v.season_year, v.season.is_some(), ranged) {
            // An exact year with a season is the season year; alone, it is the start-date year.
            (Some(y), true, _) => {
                m.insert("seasonYear".into(), y.into());
            }
            (Some(y), false, false) => {
                m.insert("startDate_greater".into(), (y * 10000).into());
                m.insert("startDate_lesser".into(), ((y + 1) * 10000).into());
            }
            _ => {}
        }
        if ranged {
            let (from, to) = match (v.year_from, v.year_to) {
                (Some(a), Some(b)) if a > b => (Some(b), Some(a)),
                other => other,
            };
            if let Some(a) = from {
                m.insert("startDate_greater".into(), (a * 10000).into());
            }
            if let Some(b) = to {
                m.insert("startDate_lesser".into(), ((b + 1) * 10000).into());
            }
        }
        let (emin, emax) = match (v.episodes_min, v.episodes_max) {
            (Some(a), Some(b)) if a > b => (Some(b), Some(a)),
            other => other,
        };
        if let Some(a) = emin.filter(|a| *a > 0) {
            m.insert("episodes_greater".into(), (a - 1).into());
        }
        if let Some(b) = emax {
            m.insert("episodes_lesser".into(), (b + 1).into());
        }
        let (smin, smax) = match (v.score_min.map(|s| s.min(100)), v.score_max.map(|s| s.min(100))) {
            (Some(a), Some(b)) if a > b => (Some(b), Some(a)),
            other => other,
        };
        if let Some(a) = smin.filter(|a| *a > 0) {
            m.insert("averageScore_greater".into(), (a - 1).into());
        }
        if let Some(b) = smax.filter(|b| *b < 100) {
            m.insert("averageScore_lesser".into(), (b + 1).into());
        }
        m.insert("sort".into(), json!([v.sort.anilist(!query.is_empty())]));
        // `isAdult: false` hides 18+; with the switch on both kinds are returned (the argument is omitted).
        if !v.adult {
            m.insert("isAdult".into(), false.into());
        }
        Value::Object(m)
    }

    /// POST without the card fragment (queries that select no `...mediaCard` would fail validation with it).
    async fn post_plain(&self, query: &str, variables: Value) -> Result<Value, ProviderError> {
        let resp = self
            .client
            .post(&self.endpoint)
            .header("Accept", "application/json")
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status == 429 {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        let body: Value = resp.json().await.map_err(|e| ProviderError::Parse(e.without_url().to_string()))?;
        if body["errors"].as_array().is_some_and(|e| e.iter().any(|x| x["status"] == 429)) {
            return Err(ProviderError::RateLimited { provider: "anilist".into() });
        }
        if !(200..300).contains(&status) && status != 404 {
            return Err(ProviderError::Http(status));
        }
        Ok(body)
    }

    /// Filtered search with the page total and the 18+ flags.
    pub async fn search_filtered_page(&self, v: &SearchVars) -> Result<SearchResultPage, ProviderError> {
        map_search_page(&self.post(SEARCH_QUERY, Self::search_vars(v)).await?)
    }

    pub async fn search_filtered(&self, v: &SearchVars) -> Result<FeedPage, ProviderError> {
        Ok(self.search_filtered_page(v).await?.page)
    }

    /// [`Self::studio`] with the total; `adult` keeps 18+ titles.
    pub async fn studio_page(&self, name: &str, page: u32, adult: bool) -> Result<SearchResultPage, ProviderError> {
        let name = name.trim();
        if name.is_empty() {
            return Ok(SearchResultPage::default());
        }
        let vars = json!({ "search": name, "page": page.max(1), "perPage": 30 });
        map_studio_page(&self.post(STUDIO_QUERY, vars).await?, adult)
    }

    /// Titles of a studio / producer by name, popularity first.
    pub async fn studio(&self, name: &str, page: u32) -> Result<FeedPage, ProviderError> {
        Ok(self.studio_page(name, page, false).await?.page)
    }

    /// Character page by AniList character id. `Ok(None)` when unknown.
    pub async fn character(&self, id: u32) -> Result<Option<Character>, ProviderError> {
        if id == 0 {
            return Ok(None);
        }
        map_character(&self.post(&character_query(), json!({ "id": id })).await?)
    }

    /// Anime trending since `since_unix` (AniList `mediaTrends`, highest first).
    pub async fn trending_since(&self, since_unix: i64, page: u32, adult: bool) -> Result<SearchResultPage, ProviderError> {
        let vars = json!({ "page": page.max(1), "perPage": 50, "date_greater": since_unix.max(0) });
        map_trends_page(&self.post(TRENDS_QUERY, vars).await?, adult)
    }

    /// Titles by AniList id, in the order of `ids` (unknown ids are skipped).
    pub async fn media_by_ids(&self, ids: &[u32]) -> Result<Vec<Media>, ProviderError> {
        let ids: Vec<u32> = ids.iter().copied().filter(|id| *id != 0).take(50).collect();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let page = map_anilist_page(&self.post(IDS_QUERY, json!({ "ids": ids, "perPage": ids.len() })).await?)?;
        let mut items = page.items;
        items.sort_by_key(|m| m.media_ref.anilist_id.and_then(|id| ids.iter().position(|x| *x == id)).unwrap_or(usize::MAX));
        Ok(items)
    }

    /// AniList's tag collection (for the tag filter).
    pub async fn tag_collection(&self) -> Result<Vec<MediaTag>, ProviderError> {
        map_tags(&self.post_plain(TAGS_QUERY, json!({})).await?)
    }

    /// Characters by name, best match first.
    pub async fn search_characters(&self, query: &str, page: u32) -> Result<CharacterPage, ProviderError> {
        let q = query.split_whitespace().collect::<Vec<_>>().join(" ");
        if q.is_empty() {
            return Ok(CharacterPage { page: 1, ..Default::default() });
        }
        map_character_page(&self.post_plain(CHARACTER_SEARCH_QUERY, json!({ "search": q, "page": page.max(1), "perPage": 30 })).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> SearchVars {
        SearchVars { kind: Some(MediaKind::Anime), page: 1, per_page: 30, ..Default::default() }
    }

    #[test]
    fn plain_vars_only_page_type_sort_and_adult() {
        let v = AniList::search_vars(&vars());
        assert_eq!(v["page"], 1);
        assert_eq!(v["perPage"], 30);
        assert_eq!(v["type"], "ANIME");
        assert_eq!(v["sort"], json!(["POPULARITY_DESC"]));
        assert_eq!(v["isAdult"], false);
        assert!(v.get("search").is_none());
        assert!(v.get("genre_in").is_none());
        assert!(vars().is_plain());
    }

    #[test]
    fn query_uses_search_match_and_is_normalized() {
        let v = AniList::search_vars(&SearchVars { query: "  one   piece ".into(), ..vars() });
        assert_eq!(v["search"], "one piece");
        assert_eq!(v["sort"], json!(["SEARCH_MATCH"]));
        let v = AniList::search_vars(&SearchVars { query: "naruto".into(), sort: SearchSort::Score, ..vars() });
        assert_eq!(v["sort"], json!(["SCORE_DESC"]));
    }

    #[test]
    fn filters_map_to_anilist_arguments() {
        let v = AniList::search_vars(&SearchVars {
            genres: vec!["Action".into(), "Action".into(), " ".into(), "Drama".into()],
            tags: vec!["Isekai".into()],
            format: Some("tv".into()),
            status: Some("RELEASING".into()),
            season: Some("winter".into()),
            season_year: Some(2025),
            sort: SearchSort::Trending,
            adult: true,
            page: 3,
            ..vars()
        });
        assert_eq!(v["genre_in"], json!(["Action", "Drama"]));
        assert_eq!(v["tag_in"], json!(["Isekai"]));
        assert_eq!(v["format_in"], json!(["TV"]));
        assert_eq!(v["status_in"], json!(["RELEASING"]));
        assert_eq!(v["season"], "WINTER");
        assert_eq!(v["seasonYear"], 2025);
        assert_eq!(v["sort"], json!(["TRENDING_DESC"]));
        assert_eq!(v["page"], 3);
        assert!(v.get("isAdult").is_none(), "18+ on returns both kinds");
        assert!(v.get("startDate_greater").is_none());
    }

    #[test]
    fn ranges_are_inclusive_and_swapped_when_reversed() {
        let v = AniList::search_vars(&SearchVars {
            year_from: Some(2015),
            year_to: Some(2010),
            episodes_min: Some(1),
            episodes_max: Some(13),
            score_min: Some(80),
            ..vars()
        });
        assert_eq!(v["startDate_greater"], 20100000);
        assert_eq!(v["startDate_lesser"], 20160000);
        assert_eq!(v["episodes_greater"], 0);
        assert_eq!(v["episodes_lesser"], 14);
        assert_eq!(v["averageScore_greater"], 79);
        assert!(v.get("averageScore_lesser").is_none(), "a max of 100 is no bound");
        let v = AniList::search_vars(&SearchVars { season_year: Some(2020), ..vars() });
        assert_eq!(v["startDate_greater"], 20200000);
        assert_eq!(v["startDate_lesser"], 20210000);
        let v = AniList::search_vars(&SearchVars { score_min: Some(0), score_max: Some(60), episodes_min: Some(0), ..vars() });
        assert!(v.get("averageScore_greater").is_none());
        assert_eq!(v["averageScore_lesser"], 61);
        assert!(v.get("episodes_greater").is_none());
    }

    #[test]
    fn sorts_cover_every_order() {
        assert_eq!(SearchSort::Newest.anilist(false), "START_DATE_DESC");
        assert_eq!(SearchSort::Title.anilist(true), "TITLE_ROMAJI");
        assert_eq!(SearchSort::Popularity.anilist(true), "POPULARITY_DESC");
        assert_eq!(SearchSort::Relevance.anilist(false), "POPULARITY_DESC");
    }

    fn card(id: u32, kind: &str, adult: bool) -> Value {
        json!({ "id": id, "type": kind, "title": { "romaji": format!("T{id}") }, "isAdult": adult, "seasonYear": 2020 })
    }

    #[test]
    fn search_page_keeps_total_and_adult_flags() {
        let doc = json!({ "data": { "Page": {
            "pageInfo": { "total": 120, "currentPage": 2, "hasNextPage": true },
            "media": [card(1, "ANIME", false), card(2, "ANIME", true)]
        } } });
        let p = map_search_page(&doc).unwrap();
        assert_eq!(p.page.items.len(), 2);
        assert_eq!(p.page.page, 2);
        assert!(p.page.has_next);
        assert_eq!(p.total, Some(120));
        assert!(p.adult_ids.contains(&2) && !p.adult_ids.contains(&1));
    }

    #[test]
    fn trends_are_deduplicated_and_filtered() {
        let row = |id: u32, kind: &str, adult: bool| json!({ "mediaId": id, "trending": 10, "media": card(id, kind, adult) });
        let doc = json!({ "data": { "Page": {
            "pageInfo": { "currentPage": 1, "hasNextPage": false },
            "mediaTrends": [row(1, "ANIME", false), row(1, "ANIME", false), row(2, "MANGA", false), row(3, "ANIME", true)]
        } } });
        let p = map_trends_page(&doc, false).unwrap();
        assert_eq!(p.page.items.iter().filter_map(|m| m.media_ref.anilist_id).collect::<Vec<_>>(), vec![1]);
        let p = map_trends_page(&doc, true).unwrap();
        assert_eq!(p.page.items.len(), 2);
        assert!(p.adult_ids.contains(&3));
    }

    #[test]
    fn studio_page_maps_nodes_and_unknown_studio() {
        let doc = json!({ "data": { "Studio": { "id": 1, "name": "MAPPA", "media": {
            "pageInfo": { "total": 2, "currentPage": 1, "hasNextPage": false },
            "nodes": [card(5, "ANIME", false), card(6, "ANIME", true), card(7, "MANGA", false)]
        } } } });
        let p = map_studio_page(&doc, false).unwrap();
        assert_eq!(p.page.items.len(), 1);
        assert_eq!(p.total, Some(2));
        assert_eq!(map_studio_page(&doc, true).unwrap().page.items.len(), 2);
        let none = json!({ "data": { "Studio": null }, "errors": [{ "message": "Not Found.", "status": 404 }] });
        assert!(map_studio_page(&none, false).unwrap().page.items.is_empty());
        let bad = json!({ "data": null, "errors": [{ "message": "Validation error", "status": 400 }] });
        assert!(map_studio_page(&bad, false).is_err());
    }

    #[test]
    fn character_maps_names_dates_and_voice_actors() {
        let va = |id: u32, lang: &str| json!({ "id": id, "name": { "full": format!("VA{id}") }, "image": { "medium": "v.png" }, "languageV2": lang });
        let doc = json!({ "data": { "Character": {
            "id": 40, "name": { "full": "Luffy", "native": "ルフィ", "alternative": ["Straw Hat", "Luffy", ""] },
            "image": { "large": "l.png" }, "description": "__Captain__ of the crew. ~!He is secretly X.!~ See [this](https://x).",
            "gender": "Male", "age": "17-19", "dateOfBirth": { "year": null, "month": 5, "day": 5 }, "bloodType": "F",
            "favourites": 5000,
            "media": { "edges": [
                { "characterRole": "MAIN", "ja": [va(1, "Japanese")], "en": [va(2, "English"), va(2, "English")], "es": [],
                  "node": card(21, "ANIME", false) },
                { "characterRole": "MAIN", "node": { "id": 0 } }
            ] }
        } } });
        let c = map_character(&doc).unwrap().unwrap();
        assert_eq!(c.id, 40);
        assert_eq!(c.native_name.as_deref(), Some("ルフィ"));
        assert_eq!(c.alternative_names, vec!["Straw Hat".to_owned()]);
        assert_eq!(c.birthday, (None, Some(5), Some(5)));
        assert_eq!(c.description.as_deref(), Some("Captain of the crew.  See this."));
        assert_eq!(c.appearances.len(), 1);
        assert_eq!(c.appearances[0].role, "MAIN");
        assert_eq!(c.appearances[0].voice_actors.len(), 2);
        assert_eq!(c.appearances[0].voice_actors[1].role.as_deref(), Some("English"));
        let missing = json!({ "data": { "Character": null }, "errors": [{ "message": "Not Found.", "status": 404 }] });
        assert_eq!(map_character(&missing).unwrap(), None);
    }

    #[test]
    fn character_query_selects_every_language_and_the_card_fragment() {
        let q = character_query();
        for (alias, lang) in VOICE_LANGUAGES {
            assert!(q.contains(&format!("{alias}: voiceActors(language: {lang}")));
        }
        assert!(q.contains("...mediaCard"));
        assert_eq!(q.matches('{').count(), q.matches('}').count());
    }

    #[test]
    fn tags_and_character_hits_map() {
        let doc = json!({ "data": { "MediaTagCollection": [
            { "name": "Twist", "category": "Theme", "isGeneralSpoiler": true, "isAdult": false },
            { "name": "Isekai", "category": "Setting", "isGeneralSpoiler": false, "isAdult": false },
            { "name": "", "category": "X" }
        ] } });
        let tags = map_tags(&doc).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].name, "Isekai");
        assert!(tags[1].spoiler);
        let doc = json!({ "data": { "Page": { "pageInfo": { "total": 1, "currentPage": 1, "hasNextPage": false },
            "characters": [{ "id": 40, "name": { "full": "Luffy", "native": "ルフィ" }, "image": { "large": "l.png" }, "favourites": 3 }, { "id": 0 }] } } });
        let p = map_character_page(&doc).unwrap();
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].image.as_deref(), Some("l.png"));
        assert_eq!(p.total, Some(1));
    }

    #[test]
    fn description_cleaning_handles_unclosed_spoilers() {
        assert_eq!(clean_character_description(Some("Shown ~!hidden")).as_deref(), Some("Shown"));
        assert_eq!(clean_character_description(None), None);
        assert_eq!(clean_character_description(Some("~!all hidden!~")), None);
    }
}
