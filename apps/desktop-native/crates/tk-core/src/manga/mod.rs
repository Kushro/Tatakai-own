//! Manga chapters across providers (wave B, worker **l**; docs/redesign/16-parity-wave-b-contract.md section 3.5;
//! checklist `manga.chapters.providers`, `manga.chapters.auto-best`, `manga.metadata.*`). UI-free. l owns this
//! directory and adds private files.
//!
//! - [`parse_ext_chapters`] / [`parse_ext_pages`]: the `manga/chapters` / `manga/pages` routes of a `.kai`
//!   extension (classic contract `streaming-sources-v3`: chapters merged by number, each with one source per inner
//!   provider; `chapterKey` = `<provider>:<providerChapterId>`). An extension with several inner providers (Toko:
//!   MangaDex, Comick, Mangapill...) becomes one [`ChapterList`] per inner provider, like the classic
//!   `MangaChapterProviderRows`.
//! - [`merge`]: one chapter per number across lists, the preferred language / source / most reliable first; the
//!   reader's chapter navigator and "next chapter" use it.
//! - [`pick_best`] / [`best_list`]: the auto-best source of a chapter / of a title (`data-best`).
//!
//! Metadata providers: `crate::metadata::{mangadex, mangaupdates}` (manga-only `MetadataProvider`s registered by
//! `MetadataRouter::with_defaults`) and `crate::metadata::anilist::manga` (AniList `MANGA` queries incl. `NOVEL`
//! format for worker m). Rows: `store/reading.rs`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One chapter of one provider (the extension's own `chapter_id` is opaque).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub ext_id: String,
    pub chapter_id: String,
    pub number: f32,
    pub volume: Option<u32>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub group: Option<String>,
    pub published_unix: Option<i64>,
    pub pages: Option<u32>,
}

/// The chapters of one provider of one title: `ext_id` is the extension (or the built-in `@mangadex`), `provider`
/// its inner provider name (equal to `ext_id` when the extension has none), `lang` the dominant language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterList {
    pub ext_id: String,
    pub provider: String,
    pub lang: Option<String>,
    pub chapters: Vec<Chapter>,
}

/// One page of a chapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub index: u32,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Language codes the providers use, folded to ISO 639-1 (`es-la` -> `es`, `pt-br` -> `pt`, `Spanish` -> `es`).
pub fn normalize_lang(raw: &str) -> Option<String> {
    let l = raw.trim().to_ascii_lowercase();
    if l.is_empty() || l == "unknown" || l == "null" {
        return None;
    }
    let named = match l.as_str() {
        "english" => Some("en"),
        "spanish" | "español" | "espanol" | "latino" => Some("es"),
        "portuguese" | "português" | "portugues" => Some("pt"),
        "french" | "français" | "francais" => Some("fr"),
        "japanese" => Some("ja"),
        "korean" => Some("ko"),
        "chinese" => Some("zh"),
        "indonesian" => Some("id"),
        "italian" => Some("it"),
        "german" | "deutsch" => Some("de"),
        "russian" => Some("ru"),
        "arabic" => Some("ar"),
        "turkish" => Some("tr"),
        "vietnamese" => Some("vi"),
        "thai" => Some("th"),
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_owned());
    }
    let base = l.split(['-', '_']).next().unwrap_or(&l);
    (base.len() == 2 && base.chars().all(|c| c.is_ascii_lowercase())).then(|| base.to_owned())
}

/// A chapter number from a JSON number or a string (`"12"`, `"12.5"`, `"Ch. 12"`); 0.0 when unknown.
pub fn parse_number(v: &Value) -> f32 {
    match v {
        Value::Number(n) => n.as_f64().map_or(0.0, |f| f as f32),
        Value::String(s) => number_in(s).unwrap_or(0.0),
        _ => 0.0,
    }
}

/// First decimal number in `s`.
fn number_in(s: &str) -> Option<f32> {
    let start = s.find(|c: char| c.is_ascii_digit())?;
    let rest = &s[start..];
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    rest[..end].trim_end_matches('.').parse::<f32>().ok()
}

/// Volume from a number or a string (`"3"`, `"Vol. 3"`); `None` when missing or 0.
pub fn parse_volume(v: &Value) -> Option<u32> {
    let n = match v {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => f64::from(number_in(s)?),
        _ => return None,
    };
    (n >= 1.0 && n < 100_000.0).then_some(n as u32)
}

/// `2024-01-05`, `2024-01-05T10:00:00Z` (any time / offset is ignored: day precision is enough for a list) or a
/// unix timestamp in seconds / milliseconds.
pub fn parse_date(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => {
            let x = n.as_i64()?;
            if x <= 0 {
                None
            } else if x > 100_000_000_000 {
                Some(x / 1000)
            } else {
                Some(x)
            }
        }
        Value::String(s) => {
            let s = s.trim();
            let date = s.get(..10)?;
            let mut it = date.split('-');
            let y: i32 = it.next()?.parse().ok()?;
            let m: u8 = it.next()?.parse().ok()?;
            let d: u8 = it.next()?.parse().ok()?;
            let month = time::Month::try_from(m).ok()?;
            let day = time::Date::from_calendar_date(y, month, d).ok()?;
            Some(day.midnight().assume_utc().unix_timestamp())
        }
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The array of entries of a `manga/chapters` answer: a bare array, `results` (debug variant) or `chapters`.
fn entries(body: &Value) -> &[Value] {
    if let Some(a) = body.as_array() {
        return a;
    }
    for key in ["results", "chapters", "data"] {
        if let Some(a) = body[key].as_array() {
            return a;
        }
    }
    &[]
}

/// The provider part of a `chapterKey` (`mangadex:abc` -> `mangadex`).
fn key_provider(key: &str) -> Option<&str> {
    key.split_once(':').map(|(p, _)| p).filter(|p| !p.is_empty())
}

/// Most frequent language of a list (ties: the first seen).
fn dominant_lang(chapters: &[Chapter]) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for c in chapters {
        if let Some(l) = &c.lang {
            match counts.iter_mut().find(|(x, _)| x == l) {
                Some((_, n)) => *n += 1,
                None => counts.push((l.clone(), 1)),
            }
        }
    }
    let max = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == max).map(|(l, _)| l)
}

/// Push `ch` into the list of `provider`, creating it on first use (order of first appearance is kept).
fn push(lists: &mut Vec<ChapterList>, ext_id: &str, provider: &str, ch: Chapter) {
    match lists.iter_mut().find(|l| l.provider == provider) {
        Some(l) => l.chapters.push(ch),
        None => lists.push(ChapterList { ext_id: ext_id.to_owned(), provider: provider.to_owned(), lang: None, chapters: vec![ch] }),
    }
}

/// Sort ascending by number, drop exact duplicates (same chapter id) and set the dominant language.
fn finish(mut lists: Vec<ChapterList>) -> Vec<ChapterList> {
    for l in &mut lists {
        l.chapters.sort_by(|a, b| a.number.total_cmp(&b.number));
        let mut seen = std::collections::HashSet::new();
        l.chapters.retain(|c| seen.insert(c.chapter_id.clone()));
        l.lang = dominant_lang(&l.chapters);
    }
    lists.retain(|l| !l.chapters.is_empty());
    lists
}

/// The `manga/chapters` answer of extension `ext_id`, one list per inner provider. Two shapes are accepted: the
/// classic merged entries (`{ number, title, volume, sources: [{ provider, chapterKey, language, scanlator,
/// releaseDate }] }`) and flat entries (`{ chapterKey | id, number, title, volume, language | lang, scanlator |
/// group, releaseDate | date, pages }`), whose provider is the `chapterKey` prefix or the extension itself.
pub fn parse_ext_chapters(ext_id: &str, body: &Value) -> Vec<ChapterList> {
    let mut lists: Vec<ChapterList> = Vec::new();
    for e in entries(body) {
        let number = parse_number(&e["number"]).max(parse_number(&e["chapter"]));
        let volume = parse_volume(&e["volume"]);
        let title = text(&e["title"]);
        if let Some(sources) = e["sources"].as_array() {
            for s in sources {
                let Some(key) = text(&s["chapterKey"]).or_else(|| text(&s["id"])) else { continue };
                let provider = text(&s["provider"]).or_else(|| key_provider(&key).map(str::to_owned)).unwrap_or_else(|| ext_id.to_owned());
                let ch = Chapter {
                    ext_id: ext_id.to_owned(),
                    chapter_id: key,
                    number,
                    volume: parse_volume(&s["volume"]).or(volume),
                    title: text(&s["title"]).or_else(|| title.clone()),
                    lang: text(&s["language"]).or_else(|| text(&s["lang"])).and_then(|l| normalize_lang(&l)),
                    group: text(&s["scanlator"]).or_else(|| text(&s["group"])),
                    published_unix: parse_date(&s["releaseDate"]).or_else(|| parse_date(&s["date"])),
                    pages: s["pages"].as_u64().and_then(|p| u32::try_from(p).ok()),
                };
                push(&mut lists, ext_id, &provider, ch);
            }
        } else if let Some(key) = text(&e["chapterKey"]).or_else(|| text(&e["id"])) {
            let provider = text(&e["provider"]).or_else(|| key_provider(&key).map(str::to_owned)).unwrap_or_else(|| ext_id.to_owned());
            let ch = Chapter {
                ext_id: ext_id.to_owned(),
                chapter_id: key,
                number,
                volume,
                title,
                lang: text(&e["language"]).or_else(|| text(&e["lang"])).and_then(|l| normalize_lang(&l)),
                group: text(&e["scanlator"]).or_else(|| text(&e["group"])),
                published_unix: parse_date(&e["releaseDate"]).or_else(|| parse_date(&e["date"])),
                pages: e["pages"].as_u64().and_then(|p| u32::try_from(p).ok()),
            };
            push(&mut lists, ext_id, &provider, ch);
        }
    }
    finish(lists)
}

/// The `manga/pages` answer: a bare array or `{ pages: [...] }` of `{ pageNumber, imageUrl | url | img, width,
/// height, headers: { Referer } }`. Pages are re-indexed from 0 in `pageNumber` order; entries without a URL are
/// dropped.
pub fn parse_ext_pages(body: &Value) -> Vec<Page> {
    let list = body.as_array().map(Vec::as_slice).or_else(|| body["pages"].as_array().map(Vec::as_slice)).unwrap_or(&[]);
    let mut out: Vec<(f64, Page)> = list
        .iter()
        .enumerate()
        .filter_map(|(i, p)| {
            let url = match p {
                Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
                _ => text(&p["imageUrl"]).or_else(|| text(&p["url"])).or_else(|| text(&p["img"])),
            }?;
            let order = p["pageNumber"].as_f64().or_else(|| p["page"].as_f64()).unwrap_or(i as f64);
            let mut headers: Vec<(String, String)> = p["headers"]
                .as_object()
                .map(|h| h.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned()))).collect())
                .unwrap_or_default();
            headers.sort();
            let dim = |v: &Value| v.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n > 0);
            Some((order, Page { index: 0, url, headers, width: dim(&p["width"]), height: dim(&p["height"]) }))
        })
        .collect();
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter()
        .enumerate()
        .map(|(i, (_, mut p))| {
            p.index = i as u32;
            p
        })
        .collect()
}

/// Ranking inputs shared by [`merge`], [`pick_best`] and [`best_list`].
#[derive(Debug, Clone, Default)]
pub struct Preference<'a> {
    pub lang: &'a str,
    /// `ext_id` or `ext_id/provider`.
    pub source: Option<&'a str>,
}

/// Whether `pref.source` names this list (`ext_id` alone matches every inner provider).
fn source_matches(list: &ChapterList, source: Option<&str>) -> bool {
    match source {
        Some(s) => s == list.ext_id || s == format!("{}/{}", list.ext_id, list.provider),
        None => false,
    }
}

/// Score of a chapter of `list` (higher is better): preferred source, then preferred language, then English as the
/// universal fallback, then reliability (0..=1), then a known group.
fn score(list: &ChapterList, ch: &Chapter, pref: &Preference<'_>, reliability: f32) -> f32 {
    let mut s = reliability.clamp(0.0, 1.0);
    if source_matches(list, pref.source) {
        s += 100.0;
    }
    match ch.lang.as_deref() {
        Some(l) if l == pref.lang => s += 10.0,
        Some("en") => s += 3.0,
        None => s += 1.0,
        _ => {}
    }
    if ch.group.is_some() {
        s += 0.1;
    }
    s
}

/// Same chapter number (fractional chapters compare exactly to the hundredth).
pub fn same_number(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.005
}

/// One chapter per number across every list, ascending, each the best candidate for `pref` (`reliability` by
/// `list.ext_id`). Chapters numbered 0 (unknown) are kept per list as they come, after the numbered ones.
pub fn merge(lists: &[ChapterList], pref: &Preference<'_>, reliability: &dyn Fn(&str) -> f32) -> Vec<Chapter> {
    let mut best: Vec<(f32, Chapter)> = Vec::new();
    let mut unnumbered = Vec::new();
    for list in lists {
        let r = reliability(&list.ext_id);
        for ch in &list.chapters {
            if ch.number <= 0.0 {
                unnumbered.push(ch.clone());
                continue;
            }
            let sc = score(list, ch, pref, r);
            match best.iter_mut().find(|(_, c)| same_number(c.number, ch.number)) {
                Some(slot) if sc > slot.0 => *slot = (sc, ch.clone()),
                Some(_) => {}
                None => best.push((sc, ch.clone())),
            }
        }
    }
    best.sort_by(|a, b| a.1.number.total_cmp(&b.1.number));
    best.into_iter().map(|(_, c)| c).chain(unnumbered).collect()
}

/// Best source of chapter `number`: `(ext_id, chapter_id)`.
pub fn pick_best(lists: &[ChapterList], number: f32, pref: &Preference<'_>, reliability: &dyn Fn(&str) -> f32) -> Option<(String, String)> {
    lists
        .iter()
        .flat_map(|l| {
            let r = reliability(&l.ext_id);
            l.chapters.iter().filter(move |c| same_number(c.number, number)).map(move |c| (score(l, c, pref, r), c))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| (c.ext_id.clone(), c.chapter_id.clone()))
}

/// Index of the best list of a title for `pref`: preferred source, then the most chapters in the preferred
/// language, then the most chapters, then reliability.
pub fn best_list(lists: &[ChapterList], pref: &Preference<'_>, reliability: &dyn Fn(&str) -> f32) -> Option<usize> {
    let key = |l: &ChapterList| {
        let in_lang = l.chapters.iter().filter(|c| c.lang.as_deref() == Some(pref.lang)).count();
        (source_matches(l, pref.source), in_lang, l.chapters.len(), (reliability(&l.ext_id) * 1000.0) as i64)
    };
    lists.iter().enumerate().max_by(|(ia, a), (ib, b)| key(a).cmp(&key(b)).then(ib.cmp(ia))).map(|(i, _)| i)
}

/// The chapter after / before `number` in a merged, ascending list.
pub fn neighbour(merged: &[Chapter], number: f32, forward: bool) -> Option<&Chapter> {
    if forward {
        merged.iter().filter(|c| c.number > 0.0).find(|c| c.number > number + 0.004)
    } else {
        merged.iter().filter(|c| c.number > 0.0).rev().find(|c| c.number < number - 0.004)
    }
}

/// Highest known chapter number of a set of lists (0.0 when none).
pub fn latest_number(lists: &[ChapterList]) -> f32 {
    lists.iter().flat_map(|l| l.chapters.iter()).map(|c| c.number).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn toko() -> Value {
        json!([
            { "number": 1, "title": "Start", "volume": "1", "sources": [
                { "provider": "mangadex", "chapterKey": "mangadex:a1", "providerChapterId": "a1", "language": "es-la", "scanlator": "Team A", "releaseDate": "2024-01-05T10:00:00Z" },
                { "provider": "comick", "chapterKey": "comick:c1", "providerChapterId": "c1", "language": "en", "scanlator": null, "releaseDate": null }
            ]},
            { "number": "2", "title": null, "volume": null, "sources": [
                { "provider": "comick", "chapterKey": "comick:c2", "providerChapterId": "c2", "language": "en", "scanlator": "X", "releaseDate": "2024-02-01" }
            ]},
            { "number": 1.5, "title": "Extra", "volume": null, "sources": [
                { "provider": "mangadex", "chapterKey": "mangadex:a15", "providerChapterId": "a15", "language": "es", "scanlator": null, "releaseDate": null }
            ]}
        ])
    }

    #[test]
    fn langs_fold_to_iso_639_1() {
        assert_eq!(normalize_lang("es-la").as_deref(), Some("es"));
        assert_eq!(normalize_lang("pt-BR").as_deref(), Some("pt"));
        assert_eq!(normalize_lang("Spanish").as_deref(), Some("es"));
        assert_eq!(normalize_lang("unknown"), None);
        assert_eq!(normalize_lang("xyz"), None);
    }

    #[test]
    fn numbers_volumes_and_dates() {
        assert_eq!(parse_number(&json!("Ch. 12.5")), 12.5);
        assert_eq!(parse_number(&json!(3)), 3.0);
        assert_eq!(parse_number(&Value::Null), 0.0);
        assert_eq!(parse_volume(&json!("Vol. 3")), Some(3));
        assert_eq!(parse_volume(&json!(0)), None);
        assert_eq!(parse_date(&json!("1970-01-02")), Some(86_400));
        assert_eq!(parse_date(&json!(1_700_000_000_000_i64)), Some(1_700_000_000));
        assert_eq!(parse_date(&json!("nope")), None);
    }

    #[test]
    fn merged_entries_split_per_inner_provider() {
        let lists = parse_ext_chapters("tatakai.extension.toko", &toko());
        assert_eq!(lists.len(), 2);
        let md = &lists[0];
        assert_eq!(md.provider, "mangadex");
        assert_eq!(md.chapters.iter().map(|c| c.number).collect::<Vec<_>>(), vec![1.0, 1.5]);
        assert_eq!(md.lang.as_deref(), Some("es"));
        assert_eq!(md.chapters[0].group.as_deref(), Some("Team A"));
        assert_eq!(md.chapters[0].volume, Some(1));
        assert!(md.chapters[0].published_unix.is_some());
        let ck = &lists[1];
        assert_eq!(ck.provider, "comick");
        assert_eq!(ck.chapters.len(), 2);
        assert_eq!(ck.lang.as_deref(), Some("en"));
        assert_eq!(ck.chapters[0].ext_id, "tatakai.extension.toko");
    }

    #[test]
    fn flat_and_debug_shapes() {
        let flat = json!({ "results": [ { "chapterKey": "x:9", "number": 9, "lang": "fr" }, { "id": "plain", "number": 10 } ] });
        let lists = parse_ext_chapters("ext", &flat);
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[0].provider, "x");
        assert_eq!(lists[1].provider, "ext");
        assert!(parse_ext_chapters("ext", &json!({ "error": "x" })).is_empty());
    }

    #[test]
    fn pages_are_ordered_and_reindexed() {
        let body = json!({ "pages": [
            { "pageNumber": 2, "imageUrl": "https://c/2.jpg", "headers": { "Referer": "https://site/" } },
            { "pageNumber": 1, "imageUrl": "https://c/1.jpg", "width": 800, "height": 0 },
            { "pageNumber": 3 }
        ]});
        let pages = parse_ext_pages(&body);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].url, "https://c/1.jpg");
        assert_eq!(pages[0].index, 0);
        assert_eq!(pages[0].width, Some(800));
        assert_eq!(pages[0].height, None);
        assert_eq!(pages[1].headers, vec![("Referer".to_owned(), "https://site/".to_owned())]);
        assert_eq!(parse_ext_pages(&json!(["https://a", " "])).len(), 1);
    }

    #[test]
    fn merge_prefers_language_then_source_then_reliability() {
        let lists = parse_ext_chapters("toko", &toko());
        let rel = |_: &str| 0.5;
        let es = Preference { lang: "es", source: None };
        let merged = merge(&lists, &es, &rel);
        assert_eq!(merged.iter().map(|c| c.number).collect::<Vec<_>>(), vec![1.0, 1.5, 2.0]);
        assert_eq!(merged[0].chapter_id, "mangadex:a1");
        let en = Preference { lang: "en", source: None };
        assert_eq!(merge(&lists, &en, &rel)[0].chapter_id, "comick:c1");
        let forced = Preference { lang: "es", source: Some("toko/comick") };
        assert_eq!(pick_best(&lists, 1.0, &forced, &rel), Some(("toko".into(), "comick:c1".into())));
        assert_eq!(pick_best(&lists, 7.0, &forced, &rel), None);
    }

    #[test]
    fn best_list_and_neighbours() {
        let lists = parse_ext_chapters("toko", &toko());
        let rel = |_: &str| 0.5;
        assert_eq!(best_list(&lists, &Preference { lang: "es", source: None }, &rel), Some(0));
        assert_eq!(best_list(&lists, &Preference { lang: "en", source: None }, &rel), Some(1));
        assert_eq!(best_list(&[], &Preference::default(), &rel), None);
        let merged = merge(&lists, &Preference { lang: "es", source: None }, &rel);
        assert_eq!(neighbour(&merged, 1.0, true).map(|c| c.number), Some(1.5));
        assert_eq!(neighbour(&merged, 1.0, false).map(|c| c.number), None);
        assert_eq!(neighbour(&merged, 2.0, false).map(|c| c.number), Some(1.5));
        assert_eq!(latest_number(&lists), 2.0);
    }
}
