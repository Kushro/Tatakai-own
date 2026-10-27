//! Pure provider-payload -> core model mappers. They are the Rust twin of
//! `contracts/fixtures/metadata/reference-mapper.mjs` and are verified against `expected.json`.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::ProviderError;
use crate::model::{Episode, FeedPage, Media, MediaKind, MediaRef};

/// JS `nn()`: `undefined`/`null`/`""` -> `None`.
fn s(v: &Value) -> Option<String> {
    match v {
        Value::String(x) if !x.is_empty() => Some(x.clone()),
        _ => None,
    }
}

fn u(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

/// Numeric JSON value or numeric string -> `u32` (JS `Number(x)` + `isFinite`, integers only).
fn num_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(_) => u(v),
        Value::String(x) => x.trim().parse::<u32>().ok(),
        _ => None,
    }
}

fn is_ws(c: char) -> bool {
    c.is_whitespace()
}

/// AniList/Kitsu synopsis cleaning: `<br>` -> newline, tags stripped, trailing "(Source: ...)" /
/// "(Written by ...)" removed, 3+ newlines collapsed, trimmed.
pub fn clean_synopsis(text: Option<&str>) -> Option<String> {
    let text = text?;
    if text.is_empty() {
        return None;
    }
    // 1. <br>, <br/>, <br /> -> \n   2. strip remaining <...> tags
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '<' {
            if let Some(rel) = chars[i + 1..].iter().position(|c| *c == '>') {
                if rel > 0 {
                    let inner: String = chars[i + 1..i + 1 + rel].iter().collect();
                    let lower = inner.to_ascii_lowercase();
                    let t = lower.trim_end_matches(is_ws).trim_end_matches('/').trim_end_matches(is_ws);
                    if t == "br" {
                        out.push('\n');
                    }
                    i += rel + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    let mut out = strip_trailing_credit(&out);
    // 3+ newlines -> 2
    let mut collapsed = String::with_capacity(out.len());
    let mut run = 0;
    for c in out.drain(..) {
        if c == '\n' {
            run += 1;
            if run <= 2 {
                collapsed.push(c);
            }
        } else {
            run = 0;
            collapsed.push(c);
        }
    }
    let trimmed = collapsed.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_owned()) }
}

/// `\s*\((?:Source|Written by)[^)]*\)\s*$` (case-insensitive)
fn strip_trailing_credit(s: &str) -> String {
    let t = s.trim_end_matches(is_ws);
    if !t.ends_with(')') {
        return s.to_owned();
    }
    let lower = t.to_ascii_lowercase();
    let body_end = t.len() - 1;
    for (idx, _) in t.char_indices().filter(|(_, c)| *c == '(') {
        // lowercase may change byte lengths for exotic chars; only compare when boundaries line up
        let Some(rest) = lower.get(idx + 1..) else { continue };
        if !(rest.starts_with("source") || rest.starts_with("written by")) {
            continue;
        }
        let Some(inner) = t.get(idx + 1..body_end) else { continue };
        if !inner.contains(')') {
            return t[..idx].trim_end_matches(is_ws).to_owned();
        }
    }
    s.to_owned()
}

fn season_of_month(m: u32) -> &'static str {
    match m {
        0..=3 => "WINTER",
        4..=6 => "SPRING",
        7..=9 => "SUMMER",
        _ => "FALL",
    }
}

pub fn map_anilist_media(m: &Value) -> Media {
    let mut titles = BTreeMap::new();
    for (key, src) in [("romaji", "romaji"), ("english", "english"), ("native", "native")] {
        if let Some(t) = s(&m["title"][src]) {
            titles.insert(key.to_owned(), t);
        }
    }
    let title = s(&m["title"]["english"])
        .or_else(|| s(&m["title"]["romaji"]))
        .or_else(|| s(&m["title"]["native"]))
        .unwrap_or_default();
    Media {
        media_ref: MediaRef { anilist_id: u(&m["id"]), mal_id: u(&m["idMal"]), kitsu_id: None },
        kind: if m["type"] == "MANGA" { MediaKind::Manga } else { MediaKind::Anime },
        title,
        titles,
        cover: s(&m["coverImage"]["extraLarge"]).or_else(|| s(&m["coverImage"]["large"])),
        banner: s(&m["bannerImage"]),
        color: s(&m["coverImage"]["color"]),
        synopsis: clean_synopsis(m["description"].as_str()),
        genres: m["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect())
            .unwrap_or_default(),
        episodes: u(&m["episodes"]),
        status: s(&m["status"]),
        season: s(&m["season"]),
        year: m["seasonYear"].as_i64().and_then(|y| i32::try_from(y).ok()),
        score: u(&m["averageScore"]),
        source_provider: "anilist".into(),
    }
}

/// Maps a `{ data: { Page: { pageInfo, media } } }` document. A body with `errors` and no data is an error.
pub fn map_anilist_page(doc: &Value) -> Result<FeedPage, ProviderError> {
    let page = &doc["data"]["Page"];
    if page.is_null() {
        let msg = doc["errors"][0]["message"].as_str().unwrap_or("missing data.Page");
        return Err(ProviderError::Parse(msg.to_owned()));
    }
    let items = page["media"]
        .as_array()
        .map(|a| a.iter().filter(|m| u(&m["id"]).is_some_and(|id| id != 0)).map(map_anilist_media).collect())
        .unwrap_or_default();
    Ok(FeedPage {
        items,
        page: u(&page["pageInfo"]["currentPage"]).unwrap_or(1),
        has_next: page["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false),
    })
}

/// ani.zip regular episodes (numeric keys only), ascending. Backticks in titles are apostrophes.
pub fn map_anizip_episodes(raw: &Value) -> Vec<Episode> {
    let clean = |v: &Value| v.as_str().map(|t| t.replace('`', "'").trim().to_owned()).filter(|t| !t.is_empty());
    let mut out: Vec<Episode> = raw["episodes"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter(|(k, _)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_digit()))
                .filter_map(|(k, e)| {
                    let number = k.parse::<u32>().ok()?;
                    let t = &e["title"];
                    Some(Episode {
                        number,
                        title: clean(&t["en"]).or_else(|| clean(&t["x-jat"])).or_else(|| clean(&t["ja"])),
                        image: s(&e["image"]),
                        air_date: s(&e["airDateUtc"]).or_else(|| s(&e["airDate"])).or_else(|| s(&e["airdate"])),
                        is_filler: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|e| e.number);
    out
}

pub fn map_anizip_ids(raw: &Value) -> MediaRef {
    let m = &raw["mappings"];
    MediaRef { anilist_id: u(&m["anilist_id"]), mal_id: u(&m["mal_id"]), kitsu_id: u(&m["kitsu_id"]) }
}

fn kitsu_status(s: &str) -> Option<&'static str> {
    match s {
        "current" => Some("RELEASING"),
        "finished" => Some("FINISHED"),
        "tba" | "unreleased" | "upcoming" => Some("NOT_YET_RELEASED"),
        _ => None,
    }
}

/// Map one Kitsu anime resource (`res`) plus its `included` relations from `doc`.
pub fn map_kitsu_media(doc: &Value, res: &Value) -> Media {
    let a = &res["attributes"];
    let included = |ty: &str, rel: &str| -> Vec<&Value> {
        let ids: Vec<&str> = res["relationships"][rel]["data"]
            .as_array()
            .map(|d| d.iter().filter_map(|x| x["id"].as_str()).collect())
            .unwrap_or_default();
        ids.iter()
            .filter_map(|id| {
                doc["included"]
                    .as_array()?
                    .iter()
                    .find(|i| i["type"] == ty && i["id"].as_str() == Some(id))
            })
            .collect()
    };
    let genres: Vec<String> =
        included("genres", "genres").iter().filter_map(|g| g["attributes"]["name"].as_str().map(str::to_owned)).collect();
    let mappings = included("mappings", "mappings");
    let ext = |site: &str| -> Option<u32> {
        mappings
            .iter()
            .find(|x| x["attributes"]["externalSite"] == site)
            .and_then(|x| num_u32(&x["attributes"]["externalId"]))
    };
    let (year, season) = a["startDate"]
        .as_str()
        .and_then(|d| {
            let (y, rest) = d.split_once('-')?;
            let mth = rest.get(..2)?;
            if y.len() != 4 || !y.bytes().all(|b| b.is_ascii_digit()) || !mth.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            Some((y.parse::<i32>().ok()?, season_of_month(mth.parse().ok()?).to_owned()))
        })
        .map_or((None, None), |(y, se)| (Some(y), Some(se)));
    let rating = match &a["averageRating"] {
        Value::Null => None,
        Value::String(x) if x.is_empty() => None,
        Value::String(x) => x.parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
    .filter(|f| f.is_finite() && *f >= 0.0)
    .map(|f| f.round() as u32);

    let mut titles = BTreeMap::new();
    if let Some(t) = s(&a["titles"]["en_jp"]) {
        titles.insert("romaji".to_owned(), t);
    }
    if let Some(t) = s(&a["titles"]["en"]).or_else(|| s(&a["titles"]["en_us"])) {
        titles.insert("english".to_owned(), t);
    }
    if let Some(t) = s(&a["titles"]["ja_jp"]) {
        titles.insert("native".to_owned(), t);
    }
    let title = titles
        .get("english")
        .cloned()
        .or_else(|| s(&a["canonicalTitle"]))
        .or_else(|| titles.get("romaji").cloned())
        .unwrap_or_default();

    Media {
        media_ref: MediaRef { anilist_id: ext("anilist/anime"), mal_id: ext("myanimelist/anime"), kitsu_id: num_u32(&res["id"]) },
        kind: MediaKind::Anime,
        title,
        titles,
        cover: s(&a["posterImage"]["large"]).or_else(|| s(&a["posterImage"]["original"])),
        banner: s(&a["coverImage"]["original"]).or_else(|| s(&a["coverImage"]["large"])),
        color: None,
        synopsis: clean_synopsis(a["synopsis"].as_str().or_else(|| a["description"].as_str())),
        genres,
        episodes: u(&a["episodeCount"]),
        status: a["status"].as_str().and_then(kitsu_status).map(str::to_owned),
        season,
        year,
        score: rating,
        source_provider: "kitsu".into(),
    }
}

/// Maps a Kitsu collection to `(items, has_next)`.
pub fn map_kitsu_list(doc: &Value) -> (Vec<Media>, bool) {
    let items = doc["data"].as_array().map(|d| d.iter().map(|r| map_kitsu_media(doc, r)).collect()).unwrap_or_default();
    (items, !doc["links"]["next"].is_null() && doc["links"]["next"] != "")
}

pub fn map_kitsu_episodes(doc: &Value) -> Vec<Episode> {
    let mut out: Vec<Episode> = doc["data"]
        .as_array()
        .map(|d| {
            d.iter()
                .filter_map(|r| {
                    let a = &r["attributes"];
                    let number = num_u32(&a["number"])?;
                    let th = &a["thumbnail"];
                    Some(Episode {
                        number,
                        title: s(&a["canonicalTitle"]),
                        image: s(&th["original"]).or_else(|| s(&th["large"])).or_else(|| s(&th["medium"])),
                        air_date: s(&a["airdate"]),
                        is_filler: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|e| e.number);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synopsis_cleaning() {
        assert_eq!(clean_synopsis(Some("a<br>b<br/>c<BR />d<i>e</i>")).as_deref(), Some("a\nb\nc\nde"));
        assert_eq!(clean_synopsis(Some("Hello world.\n\n(Source: MAL Rewrite)")).as_deref(), Some("Hello world."));
        assert_eq!(clean_synopsis(Some("x (Written by someone) ")).as_deref(), Some("x"));
        assert_eq!(clean_synopsis(Some("keep (not a credit)")).as_deref(), Some("keep (not a credit)"));
        assert_eq!(clean_synopsis(Some("a<br><br><br><br>b")).as_deref(), Some("a\n\nb"));
        assert_eq!(clean_synopsis(Some("(Source: x)")), None);
        assert_eq!(clean_synopsis(Some("")), None);
        assert_eq!(clean_synopsis(None), None);
    }
}
