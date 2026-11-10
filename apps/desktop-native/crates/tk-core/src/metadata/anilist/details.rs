//! AniList "everything else" of a title (wave A, worker **c**; docs/redesign/15 section 4): relations,
//! characters with voice actors, staff, recommendations, trailer, next airing episode, studios, producers, tags,
//! synonyms, dates, duration, source, external links. Contract: [`MediaExtra`] and `AniList::media_extra` are final.
//!
//! Two queries:
//! - [`EXTRA_QUERY`] (`media_extra` / `media_extra_raw`): the details page extras. The raw `Media` object is what
//!   tk-app caches (6 h) and maps with [`map_media_extra`]; [`media_ids`] / [`media_titles`] read the ids and the
//!   three titles from the same object.
//! - [`CHAIN_QUERY`] (`chain_node_raw`): a light "card + relations" of one title, used to walk the PREQUEL / SEQUEL
//!   chain for the watch order ([`map_chain_node`], [`chain_next`], [`watch_order_steps`]).
//!
//! Both are concatenated with `CARD_FRAGMENT` by `AniList::post`, so they must use `...mediaCard`.

use std::collections::{BTreeMap, HashSet};

use serde_json::{Value, json};

use super::AniList;
use super::search::Person;
use crate::error::ProviderError;
use crate::metadata::mapping::map_anilist_media;
use crate::model::{Media, MediaKind, MediaRef};

/// `(year, month, day)` AniList fuzzy date; any part may be missing.
pub type FuzzyDate = (Option<i32>, Option<u32>, Option<u32>);

/// Details extras of one anime (by AniList id or MAL id).
pub const EXTRA_QUERY: &str = "query ($id: Int, $idMal: Int) { Media(id: $id, idMal: $idMal, type: ANIME) { \
id idMal isAdult format source season seasonYear duration title { romaji english native } \
startDate { year month day } endDate { year month day } studios { edges { isMain node { name } } } \
countryOfOrigin meanScore popularity favourites synonyms tags { name rank isMediaSpoiler } \
trailer { id site thumbnail } nextAiringEpisode { episode airingAt } \
relations { edges { relationType node { ...mediaCard } } } \
characters(sort: [ROLE, RELEVANCE, ID], perPage: 25) { edges { role \
voiceActors(language: JAPANESE, sort: [RELEVANCE, ID]) { id name { full } image { medium } languageV2 } \
node { id name { full } image { medium } } } } \
staff(sort: [RELEVANCE, ID], perPage: 12) { edges { role node { id name { full } image { medium } } } } \
recommendations(perPage: 12, sort: [RATING_DESC, ID]) { nodes { mediaRecommendation { ...mediaCard } } } \
externalLinks { site url } } }";

/// One node of the season chain: the card, its start date and its relations (cards + start dates).
pub const CHAIN_QUERY: &str = "query ($id: Int, $idMal: Int) { Media(id: $id, idMal: $idMal, type: ANIME) { \
...mediaCard startDate { year month day } relations { edges { relationType node { ...mediaCard \
startDate { year month day } } } } } }";

/// Max PREQUEL / SEQUEL hops walked in each direction by the watch order.
pub const MAX_CHAIN_HOPS: usize = 12;
/// Max side entries (side stories, recaps) added to the watch order.
pub const MAX_SIDE_ENTRIES: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    pub media: Media,
    /// AniList `MediaRelation` (`PREQUEL`, `SEQUEL`, ...).
    pub relation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterRole {
    pub id: u32,
    pub name: String,
    pub image: Option<String>,
    /// `MAIN | SUPPORTING | BACKGROUND`
    pub role: String,
    pub voice_actors: Vec<Person>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct MediaExtra {
    pub format: Option<String>,
    pub source: Option<String>,
    pub season: Option<String>,
    pub season_year: Option<i32>,
    pub duration_min: Option<u32>,
    /// `(year, month, day)` fuzzy dates.
    pub start_date: (Option<i32>, Option<u32>, Option<u32>),
    pub end_date: (Option<i32>, Option<u32>, Option<u32>),
    pub studios: Vec<String>,
    pub producers: Vec<String>,
    pub country: Option<String>,
    pub mean_score: Option<u32>,
    pub popularity: Option<u32>,
    pub favourites: Option<u32>,
    pub synonyms: Vec<String>,
    /// `(name, rank)`, spoiler tags excluded.
    pub tags: Vec<(String, u8)>,
    pub adult: bool,
    /// `(site, id, thumbnail)`
    pub trailer: Option<(String, String, Option<String>)>,
    /// `(episode, airing_at unix)`
    pub next_airing: Option<(u32, i64)>,
    pub relations: Vec<Relation>,
    pub characters: Vec<CharacterRole>,
    pub staff: Vec<Person>,
    pub recommendations: Vec<Media>,
    /// `(site, url)`
    pub external_links: Vec<(String, String)>,
}

/// A related title as the chain walk sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainEdge {
    pub relation: String,
    pub media: Media,
    /// AniList `MediaFormat` (`TV`, `MOVIE`, ...).
    pub format: Option<String>,
    pub start: FuzzyDate,
}

/// One title of the chain with its relations.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainNode {
    pub media: Media,
    pub format: Option<String>,
    pub start: FuzzyDate,
    pub edges: Vec<ChainEdge>,
}

impl ChainNode {
    pub fn id(&self) -> Option<u32> {
        self.media.media_ref.anilist_id
    }
}

/// One entry of the watch order.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchStep {
    pub media: Media,
    /// Relation that put it here: empty for the first, `SEQUEL` along the chain, `SIDE_STORY` / `SUMMARY` for side
    /// entries.
    pub relation: String,
    pub current: bool,
}

fn s(v: &Value) -> Option<String> {
    match v {
        Value::String(x) if !x.trim().is_empty() => Some(x.trim().to_owned()),
        _ => None,
    }
}

fn u(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// AniList `FuzzyDate` object -> tuple (zero / null parts are missing).
pub fn fuzzy_date(v: &Value) -> FuzzyDate {
    let year = v["year"].as_i64().and_then(|y| i32::try_from(y).ok()).filter(|y| *y > 0);
    let month = u(&v["month"]).filter(|m| (1..=12).contains(m));
    let day = u(&v["day"]).filter(|d| (1..=31).contains(d));
    (year, month, day)
}

/// Partial ISO date: `2024-10-05`, `2024-10`, `2024`; `None` without a year. A day without a month is dropped.
pub fn format_fuzzy_date(d: FuzzyDate) -> Option<String> {
    let year = d.0?;
    Some(match (d.1, d.2) {
        (Some(m), Some(day)) => format!("{year:04}-{m:02}-{day:02}"),
        (Some(m), None) => format!("{year:04}-{m:02}"),
        _ => format!("{year:04}"),
    })
}

/// Chronological key: missing parts sort after known ones of the same year; a missing year sorts last.
fn date_key(d: FuzzyDate) -> (i32, u32, u32) {
    (d.0.unwrap_or(i32::MAX), d.1.unwrap_or(13), d.2.unwrap_or(32))
}

fn person(node: &Value, role: Option<String>) -> Option<Person> {
    let id = u(&node["id"]).filter(|id| *id != 0)?;
    Some(Person { id, name: s(&node["name"]["full"]).unwrap_or_default(), image: s(&node["image"]["medium"]), role })
}

fn valid_card(node: &Value) -> bool {
    u(&node["id"]).is_some_and(|id| id != 0)
}

/// Ids of a `Media` object of [`EXTRA_QUERY`] / [`CHAIN_QUERY`].
pub fn media_ids(m: &Value) -> MediaRef {
    MediaRef { anilist_id: u(&m["id"]).filter(|id| *id != 0), mal_id: u(&m["idMal"]).filter(|id| *id != 0), kitsu_id: None }
}

/// `romaji` / `english` / `native` titles of a `Media` object (missing ones left out).
pub fn media_titles(m: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for key in ["romaji", "english", "native"] {
        if let Some(t) = s(&m["title"][key]) {
            out.insert(key.to_owned(), t);
        }
    }
    out
}

/// Title in the preferred language (`romaji` | `english` | `native`), falling back english -> romaji -> native.
/// For the title-language preference (checklist `details.title-language`; tk-app's card mapper calls it once the
/// preference exists in `Settings`).
pub fn pick_title(titles: &BTreeMap<String, String>, preferred: &str) -> Option<String> {
    titles
        .get(preferred)
        .or_else(|| titles.get("english"))
        .or_else(|| titles.get("romaji"))
        .or_else(|| titles.get("native"))
        .cloned()
}

/// Maps the `Media` object of [`EXTRA_QUERY`].
pub fn map_media_extra(m: &Value) -> MediaExtra {
    let mut studios = Vec::new();
    let mut producers = Vec::new();
    for e in arr(&m["studios"]["edges"]) {
        let Some(name) = s(&e["node"]["name"]) else { continue };
        let list = if e["isMain"].as_bool().unwrap_or(false) { &mut studios } else { &mut producers };
        if !list.contains(&name) {
            list.push(name);
        }
    }
    producers.retain(|p| !studios.contains(p));

    let mut synonyms: Vec<String> = Vec::new();
    for x in arr(&m["synonyms"]) {
        if let Some(t) = s(x)
            && !synonyms.contains(&t)
        {
            synonyms.push(t);
        }
    }

    let tags = arr(&m["tags"])
        .iter()
        .filter(|t| !t["isMediaSpoiler"].as_bool().unwrap_or(false))
        .filter_map(|t| Some((s(&t["name"])?, u8::try_from(u(&t["rank"]).unwrap_or(0).min(100)).unwrap_or(100))))
        .collect();

    let trailer = match (s(&m["trailer"]["site"]), s(&m["trailer"]["id"])) {
        (Some(site), Some(id)) => Some((site.to_ascii_lowercase(), id, s(&m["trailer"]["thumbnail"]))),
        _ => None,
    };
    let next_airing = match (u(&m["nextAiringEpisode"]["episode"]), m["nextAiringEpisode"]["airingAt"].as_i64()) {
        (Some(ep), Some(at)) if ep > 0 && at > 0 => Some((ep, at)),
        _ => None,
    };

    let relations = arr(&m["relations"]["edges"])
        .iter()
        .filter(|e| valid_card(&e["node"]))
        .map(|e| Relation {
            media: map_anilist_media(&e["node"]),
            relation: s(&e["relationType"]).unwrap_or_else(|| "OTHER".to_owned()),
        })
        .collect();

    let characters = arr(&m["characters"]["edges"])
        .iter()
        .filter_map(|e| {
            let node = &e["node"];
            let id = u(&node["id"]).filter(|id| *id != 0)?;
            let voice_actors = arr(&e["voiceActors"])
                .iter()
                .filter_map(|va| person(va, s(&va["languageV2"]).or_else(|| Some("Japanese".to_owned()))))
                .collect();
            Some(CharacterRole {
                id,
                name: s(&node["name"]["full"]).unwrap_or_default(),
                image: s(&node["image"]["medium"]),
                role: s(&e["role"]).unwrap_or_else(|| "BACKGROUND".to_owned()),
                voice_actors,
            })
        })
        .collect();

    let staff = arr(&m["staff"]["edges"]).iter().filter_map(|e| person(&e["node"], s(&e["role"]))).collect();

    let recommendations = arr(&m["recommendations"]["nodes"])
        .iter()
        .map(|n| &n["mediaRecommendation"])
        .filter(|n| valid_card(n))
        .map(map_anilist_media)
        .collect();

    let external_links = arr(&m["externalLinks"])
        .iter()
        .filter_map(|l| {
            let url = s(&l["url"])?;
            (url.starts_with("https://") || url.starts_with("http://")).then(|| (s(&l["site"]).unwrap_or_else(|| url.clone()), url))
        })
        .collect();

    MediaExtra {
        format: s(&m["format"]),
        source: s(&m["source"]),
        season: s(&m["season"]),
        season_year: m["seasonYear"].as_i64().and_then(|y| i32::try_from(y).ok()),
        duration_min: u(&m["duration"]).filter(|d| *d > 0),
        start_date: fuzzy_date(&m["startDate"]),
        end_date: fuzzy_date(&m["endDate"]),
        studios,
        producers,
        country: s(&m["countryOfOrigin"]),
        mean_score: u(&m["meanScore"]),
        popularity: u(&m["popularity"]),
        favourites: u(&m["favourites"]),
        synonyms,
        tags,
        adult: m["isAdult"].as_bool().unwrap_or(false),
        trailer,
        next_airing,
        relations,
        characters,
        staff,
        recommendations,
        external_links,
    }
}

/// Maps the `Media` object of [`CHAIN_QUERY`]; `None` without a valid id.
pub fn map_chain_node(m: &Value) -> Option<ChainNode> {
    if !valid_card(m) {
        return None;
    }
    let edges = arr(&m["relations"]["edges"])
        .iter()
        .filter(|e| valid_card(&e["node"]))
        .map(|e| ChainEdge {
            relation: s(&e["relationType"]).unwrap_or_else(|| "OTHER".to_owned()),
            media: map_anilist_media(&e["node"]),
            format: s(&e["node"]["format"]),
            start: fuzzy_date(&e["node"]["startDate"]),
        })
        .collect();
    Some(ChainNode { media: map_anilist_media(m), format: s(&m["format"]), start: fuzzy_date(&m["startDate"]), edges })
}

/// Format preference when a title has several prequels / sequels: the series line first, then specials, movies.
fn format_rank(f: Option<&str>) -> u8 {
    match f {
        Some("TV") => 0,
        Some("TV_SHORT") | Some("ONA") => 1,
        Some("OVA") | Some("SPECIAL") => 2,
        Some("MOVIE") => 3,
        _ => 4,
    }
}

/// The next anime along `relation` (`PREQUEL` / `SEQUEL`) not seen yet: series formats first, then the earliest
/// (`SEQUEL`) or latest (`PREQUEL`) start date.
pub fn chain_next(node: &ChainNode, relation: &str, seen: &HashSet<u32>) -> Option<u32> {
    let mut list: Vec<&ChainEdge> = node
        .edges
        .iter()
        .filter(|e| e.relation == relation && e.media.kind == MediaKind::Anime)
        .filter(|e| e.media.media_ref.anilist_id.is_some_and(|id| !seen.contains(&id)))
        .collect();
    list.sort_by(|a, b| {
        let by_date = date_key(a.start).cmp(&date_key(b.start));
        format_rank(a.format.as_deref())
            .cmp(&format_rank(b.format.as_deref()))
            .then(if relation == "PREQUEL" { by_date.reverse() } else { by_date })
    });
    list.first().and_then(|e| e.media.media_ref.anilist_id)
}

/// Relations shown as side entries of a chain title.
const SIDE_RELATIONS: [&str; 2] = ["SIDE_STORY", "SUMMARY"];

/// Watch order of `chain` (oldest first; `current` is the index of the open title): each chain title, then its
/// side stories / recaps in start-date order. Every title appears once.
pub fn watch_order_steps(chain: &[ChainNode], current: usize) -> Vec<WatchStep> {
    let mut seen: HashSet<u32> = chain.iter().filter_map(ChainNode::id).collect();
    let mut out = Vec::new();
    let mut sides = 0;
    for (i, node) in chain.iter().enumerate() {
        out.push(WatchStep {
            media: node.media.clone(),
            relation: if i == 0 { String::new() } else { "SEQUEL".to_owned() },
            current: i == current,
        });
        let mut side: Vec<&ChainEdge> = node
            .edges
            .iter()
            .filter(|e| SIDE_RELATIONS.contains(&e.relation.as_str()) && e.media.kind == MediaKind::Anime)
            .collect();
        side.sort_by_key(|e| date_key(e.start));
        for e in side {
            let Some(id) = e.media.media_ref.anilist_id else { continue };
            if sides >= MAX_SIDE_ENTRIES || !seen.insert(id) {
                continue;
            }
            sides += 1;
            out.push(WatchStep { media: e.media.clone(), relation: e.relation.clone(), current: false });
        }
    }
    out
}

/// Only the id that is known goes out: AniList answers `404 Not Found` when `idMal: null` travels next to `id`.
pub(crate) fn id_vars(r: &MediaRef) -> Value {
    match (r.anilist_id, r.mal_id) {
        (Some(id), _) => json!({ "id": id }),
        (None, Some(mal)) => json!({ "idMal": mal }),
        (None, None) => json!({}),
    }
}

impl AniList {
    /// Raw `Media` object of [`EXTRA_QUERY`]; `Ok(None)` when the title is unknown or has no AniList / MAL id.
    pub async fn media_extra_raw(&self, r: &MediaRef) -> Result<Option<Value>, ProviderError> {
        if r.anilist_id.is_none() && r.mal_id.is_none() {
            return Ok(None);
        }
        let body = self.post(EXTRA_QUERY, id_vars(r)).await?;
        let m = &body["data"]["Media"];
        Ok(if m.is_object() { Some(m.clone()) } else { None })
    }

    /// `Ok(None)` when the title is unknown.
    pub async fn media_extra(&self, r: &MediaRef) -> Result<Option<MediaExtra>, ProviderError> {
        Ok(self.media_extra_raw(r).await?.as_ref().map(map_media_extra))
    }

    /// Raw `Media` object of [`CHAIN_QUERY`]; `Ok(None)` when unknown.
    pub async fn chain_node_raw(&self, r: &MediaRef) -> Result<Option<Value>, ProviderError> {
        if r.anilist_id.is_none() && r.mal_id.is_none() {
            return Ok(None);
        }
        let body = self.post(CHAIN_QUERY, id_vars(r)).await?;
        let m = &body["data"]["Media"];
        Ok(if m.is_object() { Some(m.clone()) } else { None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: u32, kind: &str, format: &str, year: i32) -> Value {
        json!({ "id": id, "type": kind, "format": format, "title": { "romaji": format!("T{id}") },
            "startDate": { "year": year, "month": 1, "day": 1 } })
    }

    fn node(id: u32, edges: Vec<(&str, Value)>) -> ChainNode {
        let edges: Vec<Value> = edges.into_iter().map(|(r, n)| json!({ "relationType": r, "node": n })).collect();
        let mut m = card(id, "ANIME", "TV", 2000 + id as i32);
        m["relations"] = json!({ "edges": edges });
        map_chain_node(&m).unwrap()
    }

    #[test]
    fn fuzzy_dates_keep_the_known_parts() {
        assert_eq!(format_fuzzy_date((Some(2024), Some(10), Some(5))), Some("2024-10-05".into()));
        assert_eq!(format_fuzzy_date((Some(2024), Some(3), None)), Some("2024-03".into()));
        assert_eq!(format_fuzzy_date((Some(2024), None, Some(9))), Some("2024".into()));
        assert_eq!(format_fuzzy_date((None, Some(3), Some(9))), None);
        assert_eq!(fuzzy_date(&json!({ "year": 2020, "month": 0, "day": null })), (Some(2020), None, None));
        assert_eq!(fuzzy_date(&Value::Null), (None, None, None));
    }

    #[test]
    fn titles_follow_the_preference_with_fallbacks() {
        let m = json!({ "title": { "romaji": "Shingeki no Kyojin", "english": "Attack on Titan", "native": "進撃の巨人" } });
        let t = media_titles(&m);
        assert_eq!(pick_title(&t, "native").as_deref(), Some("進撃の巨人"));
        assert_eq!(pick_title(&t, "romaji").as_deref(), Some("Shingeki no Kyojin"));
        let only_romaji = media_titles(&json!({ "title": { "romaji": "R", "english": null } }));
        assert_eq!(pick_title(&only_romaji, "english").as_deref(), Some("R"));
        assert_eq!(pick_title(&BTreeMap::new(), "english"), None);
    }

    #[test]
    fn studios_split_main_and_producers() {
        let m = json!({ "studios": { "edges": [
            { "isMain": true, "node": { "name": "MAPPA" } },
            { "isMain": false, "node": { "name": "Kodansha" } },
            { "isMain": false, "node": { "name": "MAPPA" } },
            { "isMain": false, "node": { "name": "Kodansha" } }
        ] } });
        let x = map_media_extra(&m);
        assert_eq!(x.studios, vec!["MAPPA".to_owned()]);
        assert_eq!(x.producers, vec!["Kodansha".to_owned()]);
    }

    #[test]
    fn chain_prefers_series_formats() {
        let n = node(5, vec![
            ("SEQUEL", card(7, "ANIME", "MOVIE", 2006)),
            ("SEQUEL", card(6, "ANIME", "TV", 2007)),
            ("SEQUEL", card(9, "MANGA", "MANGA", 2005)),
            ("PREQUEL", card(4, "ANIME", "TV", 2004)),
        ]);
        let mut seen = HashSet::from([5]);
        assert_eq!(chain_next(&n, "SEQUEL", &seen), Some(6));
        seen.insert(6);
        assert_eq!(chain_next(&n, "SEQUEL", &seen), Some(7));
        assert_eq!(chain_next(&n, "PREQUEL", &seen), Some(4));
        seen.insert(4);
        assert_eq!(chain_next(&n, "PREQUEL", &seen), None);
    }

    #[test]
    fn watch_order_inserts_side_entries_once() {
        let a = node(1, vec![("SEQUEL", card(2, "ANIME", "TV", 2002)), ("SIDE_STORY", card(10, "ANIME", "MOVIE", 2001))]);
        let b = node(2, vec![
            ("PREQUEL", card(1, "ANIME", "TV", 2001)),
            ("SIDE_STORY", card(10, "ANIME", "MOVIE", 2001)),
            ("SUMMARY", card(12, "ANIME", "SPECIAL", 2003)),
            ("SIDE_STORY", card(11, "ANIME", "OVA", 2002)),
            ("SPIN_OFF", card(13, "ANIME", "TV", 2010)),
        ]);
        let steps = watch_order_steps(&[a, b], 1);
        let ids: Vec<u32> = steps.iter().map(|s| s.media.media_ref.anilist_id.unwrap()).collect();
        assert_eq!(ids, vec![1, 10, 2, 11, 12]);
        assert_eq!(steps[0].relation, "");
        assert_eq!(steps[2].relation, "SEQUEL");
        assert!(steps[2].current && !steps[0].current);
        assert_eq!(steps[1].relation, "SIDE_STORY");
    }

    #[test]
    fn id_vars_prefer_the_anilist_id() {
        // AniList 404s on `"idMal": null` next to `id` (seen in the wave B re-review): never send a null id
        assert_eq!(id_vars(&MediaRef::anilist(21)), json!({ "id": 21 }));
        let both = MediaRef { mal_id: Some(20), ..MediaRef::anilist(21) };
        assert_eq!(id_vars(&both), json!({ "id": 21 }));
        let mal = MediaRef { mal_id: Some(5114), ..MediaRef::default() };
        assert_eq!(id_vars(&mal), json!({ "idMal": 5114 }));
    }
}
