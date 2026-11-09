//! List import / export (wave A, worker **g**; `lists.import-export`). Import reads, from a local file:
//! - a **MyAnimeList XML export** (`<myanimelist><anime>...`, "Export" on myanimelist.net, uncompressed),
//! - an **AniList JSON** dump: the GraphQL `MediaListCollection` answer (`{data:{MediaListCollection:{lists:
//!   [{entries:[...]}]}}}`), the same without `data`, or a plain array of `MediaList` entries,
//! - the **Tatakai JSON** this module writes ([`export_json`]).
//!
//! Export writes the local list as Tatakai JSON (entries + favourites, re-importable) or CSV (one row per entry).
//! Pure text in / text out: tk-app does the file and store I/O.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sync::{RowJson, media_id_for};
use super::{STATUSES, anilist_status_to_shared, score_from_mal};
use crate::store::lists::{FavoriteRow, ListEntryRow};

/// `format` field of the Tatakai JSON export.
pub const TATAKAI_FORMAT: &str = "tatakai-lists";
pub const TATAKAI_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    MalXml,
    AniListJson,
    Tatakai,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FavoriteJson {
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

impl From<&FavoriteRow> for FavoriteJson {
    fn from(f: &FavoriteRow) -> Self {
        Self {
            media_id: f.media_id.clone(),
            title: f.title.clone(),
            poster: f.poster.clone(),
            added_at: f.added_at,
        }
    }
}

impl FavoriteJson {
    pub fn into_row(self) -> FavoriteRow {
        FavoriteRow {
            media_id: self.media_id,
            title: self.title,
            poster: self.poster,
            added_at: self.added_at,
        }
    }
}

/// What a file contained. `skipped` counts entries that could not be used (no id, unknown status); `errors` says
/// why (display-ready, English, one line per problem, at most a few).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Imported {
    pub entries: Vec<RowJson>,
    pub favorites: Vec<FavoriteJson>,
    pub skipped: u32,
    pub errors: Vec<String>,
}

const MAX_ERRORS: usize = 5;

impl Imported {
    fn skip(&mut self, why: String) {
        self.skipped += 1;
        if self.errors.len() < MAX_ERRORS {
            self.errors.push(why);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TatakaiFile {
    format: String,
    version: u32,
    #[serde(default)]
    exported_at: i64,
    #[serde(default)]
    entries: Vec<RowJson>,
    #[serde(default)]
    favorites: Vec<FavoriteJson>,
}

/// Detect the format and parse. `Err` = the file is none of the supported formats.
pub fn parse(text: &str) -> Result<(Format, Imported), String> {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with('<') {
        if !t.contains("<myanimelist") {
            return Err("not a MyAnimeList XML export".into());
        }
        return Ok((Format::MalXml, parse_mal_xml(t)));
    }
    let v: Value = serde_json::from_str(t).map_err(|e| format!("not a list file: {e}"))?;
    if v["format"] == TATAKAI_FORMAT {
        let f: TatakaiFile = serde_json::from_value(v).map_err(|e| format!("invalid Tatakai list: {e}"))?;
        let mut out = Imported {
            favorites: f.favorites,
            ..Imported::default()
        };
        for mut e in f.entries {
            if !STATUSES.contains(&e.status.as_str()) {
                out.skip(format!("{}: unknown status {}", e.title, e.status));
                continue;
            }
            if e.media_id.is_empty() {
                match media_id_for(e.anilist_id, e.mal_id) {
                    Some(id) => e.media_id = id,
                    None => {
                        out.skip(format!("{}: no AniList or MAL id", e.title));
                        continue;
                    }
                }
            }
            out.entries.push(e);
        }
        return Ok((Format::Tatakai, out));
    }
    let entries = anilist_entries(&v).ok_or_else(|| "not an AniList list export".to_owned())?;
    Ok((Format::AniListJson, parse_anilist_entries(&entries)))
}

/// The `MediaList` entries of the accepted AniList shapes.
fn anilist_entries(v: &Value) -> Option<Vec<Value>> {
    if let Some(a) = v.as_array() {
        return Some(a.clone());
    }
    let coll = if v["data"]["MediaListCollection"].is_object() {
        &v["data"]["MediaListCollection"]
    } else if v["MediaListCollection"].is_object() {
        &v["MediaListCollection"]
    } else {
        v
    };
    let lists = coll["lists"].as_array()?;
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for l in lists {
        for e in l["entries"].as_array().into_iter().flatten() {
            // An entry appears once per custom list it belongs to.
            let key = e["id"]
                .as_u64()
                .or_else(|| e["media"]["id"].as_u64())
                .or_else(|| e["mediaId"].as_u64());
            if key.is_none_or(|k| seen.insert(k)) {
                out.push(e.clone());
            }
        }
    }
    Some(out)
}

fn u32_of(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0)
}

/// `{year, month, day}` -> `YYYY-MM-DD` (missing month / day = 01); `None` without a year.
pub fn fuzzy_date(v: &Value) -> Option<String> {
    let y = v["year"].as_u64()?;
    let m = v["month"].as_u64().unwrap_or(1).clamp(1, 12);
    let d = v["day"].as_u64().unwrap_or(1).clamp(1, 31);
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

fn parse_anilist_entries(entries: &[Value]) -> Imported {
    let mut out = Imported::default();
    for e in entries {
        let m = &e["media"];
        let anilist_id = u32_of(&m["id"]).or_else(|| u32_of(&e["mediaId"]));
        let mal_id = u32_of(&m["idMal"]);
        let title = ["english", "romaji", "native"]
            .iter()
            .find_map(|k| m["title"][*k].as_str().filter(|s| !s.is_empty()))
            .unwrap_or_default()
            .to_owned();
        let Some(media_id) = media_id_for(anilist_id, mal_id) else {
            out.skip(format!("{title}: no AniList or MAL id"));
            continue;
        };
        let raw_status = e["status"].as_str().unwrap_or_default();
        let Some(status) = anilist_status_to_shared(raw_status) else {
            out.skip(format!("{title}: unknown status {raw_status}"));
            continue;
        };
        // POINT_100 dumps have scores above 10; POINT_10 / decimal dumps are scaled up.
        let score = e["score"].as_f64().filter(|s| *s > 0.0).map(|s| {
            let pct = if s > 10.0 { s.min(100.0) } else { s * 10.0 };
            pct.round() as u8
        });
        out.entries.push(RowJson {
            media_id,
            anilist_id,
            mal_id,
            title,
            poster: m["coverImage"]["large"]
                .as_str()
                .or_else(|| m["coverImage"]["medium"].as_str())
                .map(str::to_owned),
            status: status.to_owned(),
            progress: e["progress"]
                .as_u64()
                .map_or(0, |p| p.min(u64::from(u32::MAX)) as u32),
            episodes: u32_of(&m["episodes"]),
            score,
            started_at: fuzzy_date(&e["startedAt"]),
            finished_at: fuzzy_date(&e["completedAt"]),
            repeats: e["repeat"].as_u64().map_or(0, |r| r.min(1000) as u32),
            notes: e["notes"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned),
            remote_id: e["id"].as_u64(),
            updated_at: e["updatedAt"].as_i64().unwrap_or(0),
        });
    }
    out
}

/// MAL export status (text or the numeric code of older exports) -> shared status.
pub fn mal_export_status(s: &str, rewatching: bool) -> Option<&'static str> {
    let s = s.trim().to_ascii_lowercase();
    Some(match s.as_str() {
        "watching" | "1" if rewatching => "repeating",
        "watching" | "1" => "watching",
        "completed" | "2" => "completed",
        "on-hold" | "on hold" | "3" => "paused",
        "dropped" | "4" => "dropped",
        "plan to watch" | "6" => "planning",
        _ => return None,
    })
}

/// Text of `<tag>...</tag>` inside `block` (CDATA unwrapped, entities decoded).
fn xml_tag(block: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    let raw = block[start..end].trim();
    let inner = raw.strip_prefix("<![CDATA[").and_then(|r| r.strip_suffix("]]>"));
    Some(match inner {
        Some(text) => text.to_owned(),
        None => xml_unescape(raw),
    })
}

fn xml_unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(end) = tail.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let ent = &tail[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// MAL dates are `YYYY-MM-DD` with `0000-00-00` for "unknown".
fn mal_date(s: Option<String>) -> Option<String> {
    s.filter(|d| !d.is_empty() && !d.starts_with("0000"))
}

fn parse_mal_xml(text: &str) -> Imported {
    let mut out = Imported::default();
    let mut rest = text;
    while let Some(i) = rest.find("<anime>") {
        let after = &rest[i + "<anime>".len()..];
        let Some(end) = after.find("</anime>") else {
            break;
        };
        let block = &after[..end];
        rest = &after[end + "</anime>".len()..];
        let title = xml_tag(block, "series_title").unwrap_or_default();
        let Some(mal_id) = xml_tag(block, "series_animedb_id")
            .and_then(|s| s.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
        else {
            out.skip(format!("{title}: no MAL id"));
            continue;
        };
        let rewatching = xml_tag(block, "my_rewatching").is_some_and(|s| s.trim() == "1");
        let raw_status = xml_tag(block, "my_status").unwrap_or_default();
        let Some(status) = mal_export_status(&raw_status, rewatching) else {
            out.skip(format!("{title}: unknown status {raw_status}"));
            continue;
        };
        let num = |tag: &str| xml_tag(block, tag).and_then(|s| s.trim().parse::<u32>().ok());
        out.entries.push(RowJson {
            media_id: format!("mal:{mal_id}"),
            anilist_id: None,
            mal_id: Some(mal_id),
            title,
            poster: None,
            status: status.to_owned(),
            progress: num("my_watched_episodes").unwrap_or(0),
            episodes: num("series_episodes").filter(|n| *n > 0),
            score: num("my_score").and_then(|s| score_from_mal(s.min(10) as u8)),
            started_at: mal_date(xml_tag(block, "my_start_date")),
            finished_at: mal_date(xml_tag(block, "my_finish_date")),
            repeats: num("my_times_watched").unwrap_or(0),
            notes: xml_tag(block, "my_comments").filter(|s| !s.trim().is_empty()),
            remote_id: None,
            updated_at: 0,
        });
    }
    out
}

/// Tatakai JSON export of the local list (re-importable by [`parse`]).
pub fn export_json(entries: &[ListEntryRow], favorites: &[FavoriteRow], now: i64) -> String {
    let file = TatakaiFile {
        format: TATAKAI_FORMAT.to_owned(),
        version: TATAKAI_VERSION,
        exported_at: now,
        entries: entries.iter().map(RowJson::from).collect(),
        favorites: favorites.iter().map(FavoriteJson::from).collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_else(|_| "{}".to_owned())
}

pub const CSV_HEADER: &str = "media_id,anilist_id,mal_id,title,status,progress,episodes,score,started_at,finished_at,repeats,notes,updated_at";

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

fn opt<T: ToString>(v: Option<T>) -> String {
    v.map(|x| x.to_string()).unwrap_or_default()
}

/// CSV export (header + one line per entry, RFC 4180 quoting).
pub fn export_csv(entries: &[ListEntryRow]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push_str("\r\n");
    for e in entries {
        let cols = [
            csv_field(&e.media_id),
            opt(e.anilist_id),
            opt(e.mal_id),
            csv_field(&e.title),
            e.status.clone(),
            e.progress.to_string(),
            opt(e.episodes),
            opt(e.score),
            csv_field(e.started_at.as_deref().unwrap_or_default()),
            csv_field(e.finished_at.as_deref().unwrap_or_default()),
            e.repeats.to_string(),
            csv_field(e.notes.as_deref().unwrap_or_default()),
            e.updated_at.to_string(),
        ];
        out.push_str(&cols.join(","));
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lists::sync::row;

    const MAL: &str = r#"<?xml version="1.0" encoding="UTF-8" ?>
<myanimelist>
  <myinfo><user_name>me</user_name></myinfo>
  <anime>
    <series_animedb_id>21</series_animedb_id>
    <series_title><![CDATA[One Piece]]></series_title>
    <series_episodes>0</series_episodes>
    <my_watched_episodes>1000</my_watched_episodes>
    <my_start_date>2020-01-02</my_start_date>
    <my_finish_date>0000-00-00</my_finish_date>
    <my_score>9</my_score>
    <my_status>Watching</my_status>
    <my_comments><![CDATA[]]></my_comments>
    <my_times_watched>0</my_times_watched>
    <my_rewatching>0</my_rewatching>
  </anime>
  <anime>
    <series_animedb_id>1</series_animedb_id>
    <series_title>Cowboy &amp; Bebop</series_title>
    <series_episodes>26</series_episodes>
    <my_watched_episodes>26</my_watched_episodes>
    <my_score>0</my_score>
    <my_status>Completed</my_status>
    <my_times_watched>1</my_times_watched>
    <my_rewatching>0</my_rewatching>
  </anime>
  <anime>
    <series_animedb_id>5</series_animedb_id>
    <series_title>Odd</series_title>
    <my_status>Something</my_status>
  </anime>
</myanimelist>"#;

    #[test]
    fn mal_xml_export() {
        let (f, out) = parse(MAL).unwrap();
        assert_eq!(f, Format::MalXml);
        assert_eq!(out.entries.len(), 2);
        assert_eq!(out.skipped, 1);
        let op = &out.entries[0];
        assert_eq!(
            (op.media_id.as_str(), op.title.as_str(), op.status.as_str()),
            ("mal:21", "One Piece", "watching")
        );
        assert_eq!((op.progress, op.episodes, op.score), (1000, None, Some(90)));
        assert_eq!(
            (
                op.started_at.as_deref(),
                op.finished_at.as_deref(),
                op.notes.as_deref()
            ),
            (Some("2020-01-02"), None, None)
        );
        let cb = &out.entries[1];
        assert_eq!(
            (cb.title.as_str(), cb.status.as_str(), cb.score, cb.repeats),
            ("Cowboy & Bebop", "completed", None, 1)
        );
    }

    #[test]
    fn anilist_json_shapes() {
        let doc = serde_json::json!({ "data": { "MediaListCollection": { "lists": [
            { "entries": [
                { "id": 7, "status": "CURRENT", "progress": 3, "score": 85, "repeat": 0, "notes": "good",
                  "startedAt": { "year": 2024, "month": 4, "day": null }, "completedAt": {}, "updatedAt": 1700000000,
                  "media": { "id": 154587, "idMal": 52991, "episodes": 28, "title": { "romaji": "Sousou no Frieren", "english": "Frieren" },
                             "coverImage": { "large": "https://img/x.jpg" } } },
                { "id": 8, "status": "PLANNING", "progress": 0, "score": 8, "media": { "id": 1 } }
            ]},
            { "entries": [ { "id": 7, "status": "CURRENT", "media": { "id": 154587 } }, { "status": "NOPE", "media": { "id": 3 } } ] }
        ]}}});
        let (f, out) = parse(&doc.to_string()).unwrap();
        assert_eq!(f, Format::AniListJson);
        assert_eq!(out.entries.len(), 2, "duplicates across custom lists are dropped");
        assert_eq!(out.skipped, 1);
        let fr = &out.entries[0];
        assert_eq!(
            (fr.media_id.as_str(), fr.mal_id, fr.title.as_str()),
            ("154587", Some(52991), "Frieren")
        );
        assert_eq!(
            (fr.status.as_str(), fr.progress, fr.score, fr.episodes),
            ("watching", 3, Some(85), Some(28))
        );
        assert_eq!(fr.started_at.as_deref(), Some("2024-04-01"));
        assert_eq!(fr.finished_at, None);
        assert_eq!(out.entries[1].score, Some(80), "POINT_10 scores are scaled");
        let arr = serde_json::json!([{ "status": "COMPLETED", "mediaId": 5 }]);
        assert_eq!(parse(&arr.to_string()).unwrap().1.entries[0].media_id, "5");
        assert!(parse("{\"hello\":1}").is_err());
        assert!(parse("<html></html>").is_err());
        assert!(parse("garbage").is_err());
    }

    #[test]
    fn tatakai_json_round_trip_and_csv() {
        let mut a = row("local", "21", "watching", 3, 10);
        a.title = "One, \"Piece\"".into();
        let b = row("local", "mal:5", "completed", 12, 20);
        let fav = FavoriteRow {
            media_id: "character:40".into(),
            title: "Luffy".into(),
            poster: None,
            added_at: 3,
        };
        let json = export_json(&[a.clone(), b.clone()], std::slice::from_ref(&fav), 99);
        let (f, out) = parse(&json).unwrap();
        assert_eq!(f, Format::Tatakai);
        assert_eq!(
            out.entries
                .iter()
                .map(|e| e.media_id.as_str())
                .collect::<Vec<_>>(),
            ["21", "mal:5"]
        );
        assert_eq!(out.entries[0].title, a.title);
        assert_eq!(out.favorites[0].clone().into_row(), fav);
        let csv = export_csv(&[a]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some(CSV_HEADER));
        assert!(
            lines
                .next()
                .unwrap()
                .starts_with("21,21,,\"One, \"\"Piece\"\"\",watching,3,12,")
        );
    }

    #[test]
    fn xml_entities() {
        assert_eq!(
            xml_unescape("a &amp; b &lt;c&gt; &#65;&#x42; & z"),
            "a & b <c> AB & z"
        );
        assert_eq!(mal_export_status("Plan to Watch", false), Some("planning"));
        assert_eq!(mal_export_status("Watching", true), Some("repeating"));
        assert_eq!(mal_export_status("3", false), Some("paused"));
    }
}
