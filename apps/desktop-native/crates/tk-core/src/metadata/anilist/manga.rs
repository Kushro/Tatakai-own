//! AniList manga / novel queries (wave B, worker **l**; worker m reads `NOVEL` through the same functions; checklist
//! `manga.metadata.anilist`, `manga.search`, `manga.details`, `manga.readlist*`, `novel.search`, `novel.details`).
//! Child module of `anilist` (private `post`, `post_auth` of `lists.rs`).
//!
//! - [`MangaSearch`] -> [`manga_vars`] -> `AniList::manga_search`: `type: MANGA` with origin (`countryOfOrigin` JP /
//!   KR / CN / TW), status, genres, tags, format (`MANGA`, `ONE_SHOT`, `NOVEL`; novels excluded unless asked), start
//!   year, sort and the 18+ switch (`isAdult`: hidden / only adult / both).
//! - `AniList::manga_details_raw` + [`map_manga_details`]: the details page (chapters, volumes, authors, relations incl.
//!   the `ADAPTATION` anime, characters, recommendations, tags, external links), mapped through worker c's
//!   `details::map_media_extra` for the shared parts.
//! - `AniList::manga_list` (`MediaListCollection(userId, type: MANGA)`): the read list mirror; writes reuse worker g's
//!   `save_entry` / `delete_entry` (`SaveMediaListEntry` is type-agnostic, `progress` = chapters).

use serde_json::{Value, json};

use super::details::{MediaExtra, map_media_extra};
use super::lists::{RemoteEntry, remote_entry};
use super::{AniList, CARD_FRAGMENT};
use crate::error::{ListError, ProviderError};
use crate::metadata::mapping::map_anilist_media;
use crate::model::Media;

/// Extra card fields of a manga row (chapters / volumes / format / origin / 18+).
const MANGA_FIELDS: &str = "chapters volumes format countryOfOrigin isAdult";

fn page_query() -> String {
    format!(
        "query ($page: Int, $perPage: Int, $search: String, $sort: [MediaSort], $genre_in: [String], $tag_in: [String], \
$countryOfOrigin: CountryCode, $status_in: [MediaStatus], $format_in: [MediaFormat], $format_not_in: [MediaFormat], \
$startDate_greater: FuzzyDateInt, $startDate_lesser: FuzzyDateInt, $isAdult: Boolean) {{ Page(page: $page, perPage: $perPage) {{ \
pageInfo {{ total currentPage lastPage hasNextPage }} media(type: MANGA, search: $search, sort: $sort, genre_in: $genre_in, \
tag_in: $tag_in, countryOfOrigin: $countryOfOrigin, status_in: $status_in, format_in: $format_in, format_not_in: $format_not_in, \
startDate_greater: $startDate_greater, startDate_lesser: $startDate_lesser, isAdult: $isAdult) {{ ...mediaCard {MANGA_FIELDS} }} }} }}"
    )
}

fn details_query() -> String {
    format!(
        "query ($id: Int) {{ Media(id: $id, type: MANGA) {{ ...mediaCard {MANGA_FIELDS} siteUrl source \
startDate {{ year month day }} endDate {{ year month day }} meanScore popularity favourites synonyms \
tags {{ name rank isMediaSpoiler }} \
relations {{ edges {{ relationType node {{ ...mediaCard {MANGA_FIELDS} }} }} }} \
characters(sort: [ROLE, RELEVANCE, ID], perPage: 25) {{ edges {{ role node {{ id name {{ full }} image {{ medium }} }} }} }} \
staff(sort: [RELEVANCE, ID], perPage: 12) {{ edges {{ role node {{ id name {{ full }} image {{ medium }} }} }} }} \
recommendations(perPage: 12, sort: [RATING_DESC, ID]) {{ nodes {{ mediaRecommendation {{ ...mediaCard {MANGA_FIELDS} }} }} }} \
externalLinks {{ site url }} }} }}"
    )
}

const LIST_ENTRY_FIELDS: &str = "id status progress score(format: POINT_100) repeat notes updatedAt \
startedAt { year month day } completedAt { year month day } media { ...mediaCard chapters volumes format }";

fn list_query() -> String {
    format!(
        "query ($userId: Int) {{ MediaListCollection(userId: $userId, type: MANGA) {{ lists {{ entries {{ {LIST_ENTRY_FIELDS} }} }} }} }} {CARD_FRAGMENT}"
    )
}

/// A manga / novel search (every filter optional). Values: `country` in `JP | KR | CN | TW`, `status` in `releasing |
/// finished | hiatus | cancelled | upcoming`, `format` in `MANGA | ONE_SHOT | NOVEL`, `sort` an id of the UI's
/// `SearchSortVm` (`relevance | popularity | score | trending | newest | title`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MangaSearch {
    pub query: String,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub country: Option<String>,
    pub status: Option<String>,
    pub format: Option<String>,
    /// Novels only (`format_in: [NOVEL]`); otherwise novels are excluded unless `format` is `NOVEL`.
    pub novel: bool,
    pub year: Option<i32>,
    pub sort: String,
    /// `None` = no 18+; `Some(true)` = only 18+; `Some(false)` = both.
    pub adult: Option<bool>,
    pub page: u32,
    pub per_page: u32,
}

/// One manga card with the manga-only fields.
#[derive(Debug, Clone, PartialEq)]
pub struct MangaCard {
    pub media: Media,
    pub chapters: Option<u32>,
    pub volumes: Option<u32>,
    pub format: Option<String>,
    pub country: Option<String>,
    pub adult: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MangaPage {
    pub items: Vec<MangaCard>,
    pub page: u32,
    pub has_next: bool,
    pub total: Option<u32>,
}

/// Everything the details page needs.
#[derive(Debug, Clone, PartialEq)]
pub struct MangaDetails {
    pub card: MangaCard,
    pub extra: MediaExtra,
    /// Story / art staff (`Name (Story & Art)`).
    pub authors: Vec<String>,
    /// AniList id of the anime adaptation, if any.
    pub anime_id: Option<u32>,
    pub site_url: Option<String>,
    /// Relations kept as cards (to show manga / anime with their own fields).
    pub relations: Vec<(String, MangaCard, bool)>,
    pub recommendations: Vec<MangaCard>,
}

/// One entry of the manga list with the title's chapter count.
#[derive(Debug, Clone, PartialEq)]
pub struct MangaListEntry {
    pub entry: RemoteEntry,
    pub chapters: Option<u32>,
    pub format: Option<String>,
}

fn s(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned)
}

fn u(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0)
}

/// AniList `MediaSort` of a sort id.
pub fn sort_for(id: &str, has_query: bool) -> &'static str {
    match id {
        "popularity" => "POPULARITY_DESC",
        "score" => "SCORE_DESC",
        "trending" => "TRENDING_DESC",
        "newest" => "START_DATE_DESC",
        "title" => "TITLE_ROMAJI",
        _ if has_query => "SEARCH_MATCH",
        _ => "POPULARITY_DESC",
    }
}

/// AniList `MediaStatus` of a status id.
pub fn status_for(id: &str) -> Option<&'static str> {
    Some(match id.trim().to_ascii_lowercase().as_str() {
        "releasing" | "publishing" => "RELEASING",
        "finished" => "FINISHED",
        "hiatus" => "HIATUS",
        "cancelled" => "CANCELLED",
        "upcoming" | "not_yet_released" => "NOT_YET_RELEASED",
        _ => return None,
    })
}

/// GraphQL variables of a search (pure).
pub fn manga_vars(q: &MangaSearch) -> Value {
    let mut m = serde_json::Map::new();
    m.insert("page".into(), q.page.max(1).into());
    m.insert("perPage".into(), (if q.per_page == 0 { 20 } else { q.per_page.min(50) }).into());
    let query = q.query.split_whitespace().collect::<Vec<_>>().join(" ");
    if !query.is_empty() {
        m.insert("search".into(), query.clone().into());
    }
    m.insert("sort".into(), json!([sort_for(&q.sort, !query.is_empty())]));
    let clean = |list: &[String]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for x in list.iter().map(|x| x.trim()).filter(|x| !x.is_empty()) {
            if !out.iter().any(|y| y == x) {
                out.push(x.to_owned());
            }
        }
        out
    };
    let genres = clean(&q.genres);
    if !genres.is_empty() {
        m.insert("genre_in".into(), json!(genres));
    }
    let tags = clean(&q.tags);
    if !tags.is_empty() {
        m.insert("tag_in".into(), json!(tags));
    }
    if let Some(c) = q.country.as_deref().map(str::trim).filter(|c| matches!(c.to_ascii_uppercase().as_str(), "JP" | "KR" | "CN" | "TW")) {
        m.insert("countryOfOrigin".into(), c.to_ascii_uppercase().into());
    }
    if let Some(st) = q.status.as_deref().and_then(status_for) {
        m.insert("status_in".into(), json!([st]));
    }
    let format = q.format.as_deref().map(|f| f.trim().to_ascii_uppercase()).filter(|f| matches!(f.as_str(), "MANGA" | "ONE_SHOT" | "NOVEL"));
    if q.novel {
        m.insert("format_in".into(), json!(["NOVEL"]));
    } else if let Some(f) = format {
        m.insert("format_in".into(), json!([f]));
    } else {
        m.insert("format_not_in".into(), json!(["NOVEL"]));
    }
    if let Some(y) = q.year.filter(|y| (1900..=2200).contains(y)) {
        m.insert("startDate_greater".into(), (y * 10000).into());
        m.insert("startDate_lesser".into(), ((y + 1) * 10000).into());
    }
    match q.adult {
        None => {
            m.insert("isAdult".into(), false.into());
        }
        Some(true) => {
            m.insert("isAdult".into(), true.into());
        }
        Some(false) => {}
    }
    Value::Object(m)
}

/// A `Media` object with the manga fields -> card.
pub fn map_manga_card(m: &Value) -> MangaCard {
    MangaCard {
        media: map_anilist_media(m),
        chapters: u(&m["chapters"]),
        volumes: u(&m["volumes"]),
        format: s(&m["format"]),
        country: s(&m["countryOfOrigin"]),
        adult: m["isAdult"].as_bool().unwrap_or(false),
    }
}

fn valid(m: &Value) -> bool {
    u(&m["id"]).is_some()
}

/// `{ data: { Page } }` -> page. A body with `errors` and no data is an error.
pub fn map_manga_page(doc: &Value) -> Result<MangaPage, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        let msg = doc["errors"][0]["message"].as_str().unwrap_or("missing data.Page");
        return Err(ProviderError::Parse(msg.to_owned()));
    }
    let items = page["media"].as_array().map(|a| a.iter().filter(|m| valid(m)).map(map_manga_card).collect()).unwrap_or_default();
    Ok(MangaPage {
        items,
        page: u(&page["pageInfo"]["currentPage"]).unwrap_or(1),
        has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
        total: page["pageInfo"]["total"].as_u64().and_then(|n| u32::try_from(n).ok()),
    })
}

/// Story / art roles of the staff edges: `Name (role)`, story first, duplicates merged.
fn authors(m: &Value) -> Vec<String> {
    let mut out: Vec<(String, String)> = Vec::new();
    for e in m["staff"]["edges"].as_array().into_iter().flatten() {
        let (Some(name), Some(role)) = (s(&e["node"]["name"]["full"]), s(&e["role"])) else { continue };
        let lower = role.to_ascii_lowercase();
        if !(lower.contains("story") || lower.contains("art") || lower.contains("original") || lower.contains("author")) {
            continue;
        }
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some((_, r)) => {
                if !r.contains(&role) {
                    r.push_str(" / ");
                    r.push_str(&role);
                }
            }
            None => out.push((name, role)),
        }
    }
    out.into_iter().map(|(n, r)| format!("{n} ({r})")).collect()
}

/// The `Media` object of the details query -> [`MangaDetails`]; `None` without a valid id.
pub fn map_manga_details(m: &Value) -> Option<MangaDetails> {
    if !valid(m) {
        return None;
    }
    let edges = m["relations"]["edges"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let relations: Vec<(String, MangaCard, bool)> = edges
        .iter()
        .filter(|e| valid(&e["node"]))
        .map(|e| (s(&e["relationType"]).unwrap_or_else(|| "OTHER".into()), map_manga_card(&e["node"]), e["node"]["type"] == "ANIME"))
        .collect();
    let anime_id = edges
        .iter()
        .filter(|e| e["node"]["type"] == "ANIME")
        .min_by_key(|e| if e["relationType"] == "ADAPTATION" { 0 } else { 1 })
        .filter(|e| matches!(e["relationType"].as_str(), Some("ADAPTATION" | "ALTERNATIVE" | "SPIN_OFF" | "SIDE_STORY" | "OTHER")))
        .and_then(|e| u(&e["node"]["id"]));
    let recommendations = m["recommendations"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|n| &n["mediaRecommendation"])
        .filter(|n| valid(n))
        .map(map_manga_card)
        .collect();
    Some(MangaDetails {
        card: map_manga_card(m),
        extra: map_media_extra(m),
        authors: authors(m),
        anime_id,
        site_url: s(&m["siteUrl"]),
        relations,
        recommendations,
    })
}

/// Every entry of a manga `MediaListCollection` answer, once.
pub fn map_manga_list(body: &Value) -> Vec<MangaListEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for list in body["data"]["MediaListCollection"]["lists"].as_array().into_iter().flatten() {
        for e in list["entries"].as_array().into_iter().flatten() {
            if let Some(r) = remote_entry(e)
                && seen.insert(r.id)
            {
                out.push(MangaListEntry { chapters: u(&e["media"]["chapters"]), format: s(&e["media"]["format"]), entry: r });
            }
        }
    }
    out
}

impl AniList {
    /// Filtered manga / novel page.
    pub async fn manga_search(&self, q: &MangaSearch) -> Result<MangaPage, ProviderError> {
        map_manga_page(&self.post(&page_query(), manga_vars(q)).await?)
    }

    /// Raw `Media` object of the details query; `Ok(None)` when unknown.
    pub async fn manga_details_raw(&self, anilist_id: u32) -> Result<Option<Value>, ProviderError> {
        let body = self.post(&details_query(), json!({ "id": anilist_id })).await?;
        let m = &body["data"]["Media"];
        Ok(if m.is_object() { Some(m.clone()) } else { None })
    }

    /// Every manga / novel entry of the viewer (all statuses).
    pub async fn manga_list(&self, token: &str, user_id: u64) -> Result<Vec<MangaListEntry>, ListError> {
        let body = self.post_auth(Some(token), &list_query(), json!({ "userId": user_id })).await?;
        Ok(map_manga_list(&body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: u32, kind: &str, adult: bool) -> Value {
        json!({ "id": id, "type": kind, "title": { "romaji": format!("R{id}"), "english": null }, "coverImage": {},
                "chapters": 120, "volumes": 0, "format": "MANGA", "countryOfOrigin": "JP", "isAdult": adult })
    }

    #[test]
    fn vars_cover_every_filter() {
        let q = MangaSearch {
            query: "  one   piece ".into(),
            genres: vec!["Action".into(), "Action".into(), " ".into()],
            country: Some("kr".into()),
            status: Some("releasing".into()),
            year: Some(2020),
            sort: "score".into(),
            page: 2,
            ..MangaSearch::default()
        };
        let v = manga_vars(&q);
        assert_eq!(v["search"], "one piece");
        assert_eq!(v["genre_in"], json!(["Action"]));
        assert_eq!(v["countryOfOrigin"], "KR");
        assert_eq!(v["status_in"], json!(["RELEASING"]));
        assert_eq!(v["startDate_greater"], 20_200_000);
        assert_eq!(v["sort"], json!(["SCORE_DESC"]));
        assert_eq!(v["format_not_in"], json!(["NOVEL"]));
        assert_eq!(v["isAdult"], false);
        assert_eq!(v["page"], 2);
        assert_eq!(v["perPage"], 20);
    }

    #[test]
    fn novel_and_adult_switches() {
        let v = manga_vars(&MangaSearch { novel: true, adult: Some(true), country: Some("xx".into()), ..MangaSearch::default() });
        assert_eq!(v["format_in"], json!(["NOVEL"]));
        assert!(v.get("format_not_in").is_none());
        assert_eq!(v["isAdult"], true);
        assert!(v.get("countryOfOrigin").is_none());
        assert_eq!(v["sort"], json!(["POPULARITY_DESC"]));
        let both = manga_vars(&MangaSearch { adult: Some(false), query: "x".into(), ..MangaSearch::default() });
        assert!(both.get("isAdult").is_none());
        assert_eq!(both["sort"], json!(["SEARCH_MATCH"]));
    }

    #[test]
    fn page_maps_cards_with_manga_fields() {
        let doc = json!({ "data": { "Page": { "pageInfo": { "total": 2, "currentPage": 1, "hasNextPage": true },
            "media": [card(1, "MANGA", false), card(0, "MANGA", false), card(2, "MANGA", true)] } } });
        let p = map_manga_page(&doc).unwrap();
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.items[0].chapters, Some(120));
        assert_eq!(p.items[0].volumes, None);
        assert!(p.items[1].adult);
        assert!(p.has_next);
        assert!(map_manga_page(&json!({ "errors": [{ "message": "boom" }] })).is_err());
    }

    #[test]
    fn details_find_the_adaptation_and_authors() {
        let mut m = card(10, "MANGA", false);
        m["relations"] = json!({ "edges": [
            { "relationType": "SIDE_STORY", "node": card(11, "MANGA", false) },
            { "relationType": "ADAPTATION", "node": card(20, "ANIME", false) }
        ]});
        m["staff"] = json!({ "edges": [
            { "role": "Story & Art", "node": { "id": 1, "name": { "full": "Eiichiro Oda" } } },
            { "role": "Assistant", "node": { "id": 2, "name": { "full": "Someone" } } }
        ]});
        m["recommendations"] = json!({ "nodes": [ { "mediaRecommendation": card(30, "MANGA", false) }, { "mediaRecommendation": null } ] });
        let d = map_manga_details(&m).unwrap();
        assert_eq!(d.anime_id, Some(20));
        assert_eq!(d.authors, vec!["Eiichiro Oda (Story & Art)".to_owned()]);
        assert_eq!(d.relations.len(), 2);
        assert!(d.relations[1].2);
        assert_eq!(d.recommendations.len(), 1);
        assert!(map_manga_details(&json!({})).is_none());
    }

    #[test]
    fn list_entries_carry_chapters() {
        let body = json!({ "data": { "MediaListCollection": { "lists": [
            { "entries": [ { "id": 5, "status": "CURRENT", "progress": 12, "media": card(1, "MANGA", false) } ] },
            { "entries": [ { "id": 5, "status": "CURRENT", "progress": 12, "media": card(1, "MANGA", false) } ] }
        ] } } });
        let l = map_manga_list(&body);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].entry.progress, 12);
        assert_eq!(l[0].chapters, Some(120));
    }
}
