//! 0.3.1: several AniList queries in ONE request with GraphQL aliases, and the split of the answer back into
//! single-query documents (`{ data: { <Field>: ... }, errors }`) so the existing mappers read them unchanged.
//!
//! * [`AliasBatch`]: the generic builder (variables are prefixed with the alias, so parts never collide).
//! * Home: trending (with `description`, the spotlight shows it) + airing + seasonal + popular + top rated, plus
//!   upcoming and most favourite when enabled ([`AniList::home_batch`]).
//! * "Because you watched": the recommendations of several seeds ([`AniList::recommendations_of_many`]).

use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use super::airing::{DatedMedia, map_dated_page, map_recommendations};
use super::{AniList, CARD_LITE_FRAGMENT};
use crate::error::ProviderError;
use crate::metadata::mapping::map_anilist_page;
use crate::model::{FeedKind, FeedPage, Media, MediaKind};

/// One aliased field of a batched query.
#[derive(Debug, Clone, PartialEq)]
pub struct AliasPart {
    pub alias: String,
    /// The top-level field the mappers expect under `data` (`Page`, `Media`).
    pub field: &'static str,
    /// The selection after `<alias>: `, with `$name` placeholders for the part's own variables.
    pub body: String,
    /// `(name, GraphQL type, value)`; `$name` in `body` becomes `$<alias>_<name>`.
    pub vars: Vec<(String, &'static str, Value)>,
}

/// A query made of [`AliasPart`]s.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AliasBatch {
    pub parts: Vec<AliasPart>,
}

impl AliasBatch {
    pub fn push(&mut self, part: AliasPart) {
        self.parts.push(part);
    }

    /// `(query, variables)`; `fragments` is appended (e.g. the card fragment).
    pub fn build(&self, fragments: &str) -> (String, Value) {
        let mut decls = Vec::new();
        let mut vars = Map::new();
        let mut fields = Vec::new();
        for p in &self.parts {
            let mut body = p.body.clone();
            // Longest names first so `$page` never eats the start of `$perPage`-like names.
            let mut names: Vec<&(String, &str, Value)> = p.vars.iter().collect();
            names.sort_by_key(|(n, _, _)| std::cmp::Reverse(n.len()));
            for (name, ty, value) in names {
                let full = format!("{}_{name}", p.alias);
                body = replace_var(&body, name, &full);
                decls.push(format!("${full}: {ty}"));
                vars.insert(full, value.clone());
            }
            fields.push(format!("{}: {}", p.alias, body));
        }
        decls.sort();
        let head = if decls.is_empty() { "query".to_owned() } else { format!("query ({})", decls.join(", ")) };
        (format!("{head} {{ {} }} {fragments}", fields.join(" ")).trim_end().to_owned(), Value::Object(vars))
    }
}

/// Replace the variable `$name` (whole word) by `$full`.
fn replace_var(body: &str, name: &str, full: &str) -> String {
    let pat = format!("${name}");
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(i) = rest.find(&pat) {
        let after = &rest[i + pat.len()..];
        let whole = !after.starts_with(|c: char| c.is_alphanumeric() || c == '_');
        out.push_str(&rest[..i]);
        out.push('$');
        out.push_str(if whole { full } else { name });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The answer of one alias as a single-query document: `{ data: { <field>: data[alias] }, errors: [its errors] }`.
/// A failed alias has `data.<field> = null` and its errors (those whose `path` starts with the alias, or every
/// error when none has a path, e.g. a whole-query failure).
pub fn split_alias(doc: &Value, alias: &str, field: &str) -> Value {
    let errors: Vec<Value> = doc["errors"]
        .as_array()
        .map(|a| {
            let with_path: Vec<&Value> = a.iter().filter(|e| e["path"].is_array()).collect();
            a.iter()
                .filter(|e| with_path.is_empty() || e["path"][0] == alias || !e["path"].is_array())
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    let mut out = json!({ "data": { field: doc["data"][alias].clone() } });
    if !errors.is_empty() {
        out["errors"] = Value::Array(errors);
    }
    out
}

/// GraphQL type of the media `Page` variables the feeds use.
fn var_type(name: &str) -> &'static str {
    match name {
        "page" | "perPage" | "seasonYear" => "Int",
        "type" => "MediaType",
        "sort" => "[MediaSort]",
        "search" => "String",
        "format_not_in" => "[MediaFormat]",
        "status_in" => "[MediaStatus]",
        "status" => "MediaStatus",
        "season" => "MediaSeason",
        "isAdult" => "Boolean",
        "startDate_greater" | "startDate_lesser" => "FuzzyDateInt",
        _ => "String",
    }
}

/// A `Page { media(...) }` part from feed-style variables (`page`, `perPage` go to `Page`, the rest to `media`);
/// `extra` is selected next to `...mediaCard`.
pub fn media_page_part(alias: &str, vars: &Value, extra: &str) -> AliasPart {
    let obj = vars.as_object().cloned().unwrap_or_default();
    let mut args = Vec::new();
    let mut part_vars = Vec::new();
    for (k, v) in &obj {
        part_vars.push((k.clone(), var_type(k), v.clone()));
        if k != "page" && k != "perPage" {
            args.push(format!("{k}: ${k}"));
        }
    }
    let page_args = match (obj.contains_key("page"), obj.contains_key("perPage")) {
        (true, true) => "(page: $page, perPage: $perPage)",
        (true, false) => "(page: $page)",
        (false, true) => "(perPage: $perPage)",
        (false, false) => "",
    };
    AliasPart {
        alias: alias.to_owned(),
        field: "Page",
        body: format!(
            "Page{page_args} {{ pageInfo {{ total currentPage lastPage hasNextPage perPage }} media({}) {{ ...mediaCard {extra} }} }}",
            args.join(", ")
        ),
        vars: part_vars,
    }
}

/// The rows of Home in one answer. Each row has its own result: one alias can fail while the others answer.
#[derive(Debug, Clone)]
pub struct HomeBatch {
    pub trending: Result<FeedPage, ProviderError>,
    pub airing: Result<FeedPage, ProviderError>,
    pub seasonal: Result<FeedPage, ProviderError>,
    pub popular: Result<FeedPage, ProviderError>,
    pub top_rated: Result<FeedPage, ProviderError>,
    pub upcoming: Option<Result<(Vec<DatedMedia>, bool), ProviderError>>,
    pub favourite: Option<Result<(Vec<DatedMedia>, bool), ProviderError>>,
}

/// The five router feeds of Home, in order, with their alias.
pub const HOME_FEEDS: [(&str, FeedKind); 5] = [
    ("trending", FeedKind::Trending),
    ("airing", FeedKind::Airing),
    ("seasonal", FeedKind::Seasonal),
    ("popular", FeedKind::Popular),
    ("top", FeedKind::TopRated),
];

fn dated_vars(sort: &str, status: Option<&str>, adult: bool) -> Value {
    let mut v = json!({ "page": 1, "perPage": 20, "sort": [sort], "type": "ANIME" });
    if let Some(s) = status {
        v["status"] = s.into();
    }
    if !adult {
        v["isAdult"] = false.into();
    }
    v
}

/// The Home query: the feeds exactly as [`AniList::feed`] asks them (trending also selects `description` for the
/// spotlight), plus upcoming / most favourite (`isAdult startDate`) when asked.
pub fn home_query(upcoming: bool, favourite: bool, adult: bool, now: OffsetDateTime) -> (String, Value) {
    let mut b = AliasBatch::default();
    for (alias, f) in HOME_FEEDS {
        let extra = if f == FeedKind::Trending { "description(asHtml: false)" } else { "" };
        b.push(media_page_part(alias, &AniList::feed_vars(MediaKind::Anime, f, 1, now), extra));
    }
    if upcoming {
        b.push(media_page_part("upcoming", &dated_vars("POPULARITY_DESC", Some("NOT_YET_RELEASED"), adult), "isAdult startDate { year month day }"));
    }
    if favourite {
        b.push(media_page_part("favourite", &dated_vars("FAVOURITES_DESC", None, adult), "isAdult startDate { year month day }"));
    }
    b.build(CARD_LITE_FRAGMENT)
}

/// Split the Home answer. `Err` only when the answer as a whole is unusable (no `data`): then the caller falls back
/// to the per-feed path.
pub fn split_home(doc: &Value, upcoming: bool, favourite: bool) -> Result<HomeBatch, ProviderError> {
    if !doc["data"].is_object() {
        let msg = doc["errors"][0]["message"].as_str().unwrap_or("missing data");
        return Err(ProviderError::Parse(msg.to_owned()));
    }
    let page = |alias: &str| map_anilist_page(&split_alias(doc, alias, "Page"));
    let dated = |alias: &str| map_dated_page(&split_alias(doc, alias, "Page"));
    Ok(HomeBatch {
        trending: page("trending"),
        airing: page("airing"),
        seasonal: page("seasonal"),
        popular: page("popular"),
        top_rated: page("top"),
        upcoming: upcoming.then(|| dated("upcoming")),
        favourite: favourite.then(|| dated("favourite")),
    })
}

const RECS_BODY: &str = "Media(id: $id, type: ANIME) { id recommendations(page: 1, perPage: 15, sort: [RATING_DESC]) { \
nodes { rating mediaRecommendation { ...mediaCard isAdult } } } }";

/// The recommendations of several titles in one query (alias `r<i>` per id).
pub fn recommendations_query(ids: &[u32]) -> (String, Value) {
    let mut b = AliasBatch::default();
    for (i, id) in ids.iter().enumerate() {
        b.push(AliasPart {
            alias: format!("r{i}"),
            field: "Media",
            body: RECS_BODY.to_owned(),
            vars: vec![("id".into(), "Int", json!(id))],
        });
    }
    b.build(CARD_LITE_FRAGMENT)
}

/// Split of [`recommendations_query`]: one result per id, in order.
pub fn split_recommendations(doc: &Value, n: usize) -> Vec<Result<Vec<(Media, bool)>, ProviderError>> {
    (0..n).map(|i| map_recommendations(&split_alias(doc, &format!("r{i}"), "Media"))).collect()
}

impl AniList {
    /// Home in one request ([`home_query`] / [`split_home`]).
    pub async fn home_batch(&self, upcoming: bool, favourite: bool, adult: bool) -> Result<HomeBatch, ProviderError> {
        let (q, v) = home_query(upcoming, favourite, adult, OffsetDateTime::now_utc());
        split_home(&self.post_bare(&q, v).await?, upcoming, favourite)
    }

    /// The recommendations of each of `ids` in one request (same order; a title AniList does not know has none).
    pub async fn recommendations_of_many(
        &self,
        ids: &[u32],
    ) -> Result<Vec<Result<Vec<(Media, bool)>, ProviderError>>, ProviderError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let (q, v) = recommendations_query(ids);
        let doc = self.post_bare(&q, v).await?;
        if !doc["data"].is_object() {
            let msg = doc["errors"][0]["message"].as_str().unwrap_or("missing data");
            return Err(ProviderError::Parse(msg.to_owned()));
        }
        Ok(split_recommendations(&doc, ids.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(secs).unwrap()
    }

    #[test]
    fn builder_prefixes_variables_per_alias() {
        let mut b = AliasBatch::default();
        b.push(AliasPart {
            alias: "a".into(),
            field: "Media",
            body: "Media(id: $id, idMal: $idMal) { id }".into(),
            vars: vec![("id".into(), "Int", json!(1)), ("idMal".into(), "Int", json!(2))],
        });
        b.push(AliasPart { alias: "b".into(), field: "Media", body: "Media(id: $id) { id }".into(), vars: vec![("id".into(), "Int", json!(3))] });
        let (q, v) = b.build("");
        assert_eq!(q, "query ($a_id: Int, $a_idMal: Int, $b_id: Int) { a: Media(id: $a_id, idMal: $a_idMal) { id } b: Media(id: $b_id) { id } }");
        assert_eq!(v, json!({ "a_id": 1, "a_idMal": 2, "b_id": 3 }));
        assert_eq!(AliasBatch::default().build("").0, "query {  }");
    }

    #[test]
    fn home_query_has_every_row_once() {
        let (q, v) = home_query(true, false, false, ts(1_793_102_400));
        for alias in ["trending:", "airing:", "seasonal:", "popular:", "top:", "upcoming:"] {
            assert_eq!(q.matches(alias).count(), 1, "{alias} in {q}");
        }
        assert!(!q.contains("favourite:"));
        // Only the spotlight row selects the synopsis.
        assert_eq!(q.matches("description").count(), 1);
        assert!(q.contains("trending: Page(page: $trending_page, perPage: $trending_perPage)"));
        assert!(q.contains("fragment mediaCard on Media"));
        assert_eq!(v["trending_sort"], json!(["TRENDING_DESC"]));
        assert_eq!(v["seasonal_season"], "FALL");
        assert_eq!(v["seasonal_seasonYear"], 2026);
        assert_eq!(v["airing_status_in"], json!(["RELEASING"]));
        assert_eq!(v["upcoming_status"], "NOT_YET_RELEASED");
        assert_eq!(v["upcoming_isAdult"], false);
        assert!(q.contains("$upcoming_status: MediaStatus"));
        assert!(q.contains("$seasonal_season: MediaSeason"));
        // With 18+ allowed the dated rows drop the argument.
        let (_, v) = home_query(false, true, true, ts(1_793_102_400));
        assert!(v.get("favourite_isAdult").is_none());
        assert_eq!(v["favourite_sort"], json!(["FAVOURITES_DESC"]));
    }

    fn card(id: u32, title: &str) -> Value {
        json!({ "id": id, "type": "ANIME", "title": { "romaji": title }, "coverImage": { "large": "c.jpg" }, "isAdult": false,
                "startDate": { "year": 2027, "month": 1, "day": null }, "description": "Desc" })
    }

    fn page_of(items: Vec<Value>) -> Value {
        json!({ "pageInfo": { "currentPage": 1, "hasNextPage": true }, "media": items })
    }

    #[test]
    fn split_home_maps_each_row_and_isolates_a_failed_alias() {
        let doc = json!({
            "data": {
                "trending": page_of(vec![card(1, "One")]),
                "airing": page_of(vec![card(2, "Two")]),
                "seasonal": null,
                "popular": page_of(vec![card(3, "Three"), card(4, "Four")]),
                "top": page_of(vec![]),
                "upcoming": page_of(vec![card(5, "Five")]),
            },
            "errors": [{ "message": "Internal Server Error", "status": 500, "path": ["seasonal"] }]
        });
        let h = split_home(&doc, true, false).unwrap();
        assert_eq!(h.trending.as_ref().unwrap().items[0].title, "One");
        assert_eq!(h.trending.as_ref().unwrap().items[0].synopsis.as_deref(), Some("Desc"));
        assert_eq!(h.airing.as_ref().unwrap().items.len(), 1);
        assert_eq!(h.seasonal.unwrap_err(), ProviderError::Parse("Internal Server Error".into()));
        assert_eq!(h.popular.unwrap().items.len(), 2);
        assert!(h.top_rated.unwrap().items.is_empty());
        let (up, next) = h.upcoming.unwrap().unwrap();
        assert_eq!((up[0].start_date.as_deref(), next), (Some("2027-01"), true));
        assert!(h.favourite.is_none());
        // A whole-query failure is an error for the caller (fallback).
        assert!(split_home(&json!({ "data": null, "errors": [{ "message": "Max query complexity" }] }), false, false).is_err());
    }

    #[test]
    fn recommendations_batch_round_trip() {
        let (q, v) = recommendations_query(&[21, 5114]);
        assert!(q.starts_with("query ($r0_id: Int, $r1_id: Int) { r0: Media(id: $r0_id, type: ANIME)"));
        assert!(q.contains("r1: Media(id: $r1_id, type: ANIME)"));
        assert_eq!(v, json!({ "r0_id": 21, "r1_id": 5114 }));
        let rec = |id, adult| json!({ "rating": 10, "mediaRecommendation": { "id": id, "title": { "romaji": format!("T{id}") }, "isAdult": adult } });
        let doc = json!({
            "data": { "r0": { "id": 21, "recommendations": { "nodes": [rec(7, false), rec(8, true)] } }, "r1": null },
            "errors": [{ "message": "Not Found.", "status": 404, "path": ["r1"] }]
        });
        let out = split_recommendations(&doc, 2);
        let first = out[0].as_ref().unwrap();
        assert_eq!(first.iter().map(|(m, a)| (m.title.clone(), *a)).collect::<Vec<_>>(), vec![("T7".into(), false), ("T8".into(), true)]);
        // Unknown id: no recommendations, like the single query.
        assert!(out[1].as_ref().unwrap().is_empty());
    }

    #[test]
    fn split_alias_keeps_only_its_errors() {
        let doc = json!({ "data": { "a": { "id": 1 }, "b": null }, "errors": [{ "message": "x", "path": ["b", "media"] }] });
        assert!(split_alias(&doc, "a", "Media").get("errors").is_none());
        assert_eq!(split_alias(&doc, "b", "Media")["errors"][0]["message"], "x");
        // Errors without a path concern the whole query.
        let whole = json!({ "data": null, "errors": [{ "message": "boom" }] });
        assert_eq!(split_alias(&whole, "a", "Page")["errors"][0]["message"], "boom");
        assert!(split_alias(&whole, "a", "Page")["data"]["Page"].is_null());
    }

    #[test]
    fn variable_names_are_replaced_as_whole_words() {
        assert_eq!(replace_var("x($page, $perPage)", "page", "a_page"), "x($a_page, $perPage)");
        assert_eq!(replace_var("x($perPage)", "perPage", "a_perPage"), "x($a_perPage)");
    }
}
