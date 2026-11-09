//! Novel chapter text (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md section 3.5; checklist
//! `novel.chapters`, `novel.reader.*`, `novel.details.check-sources`). UI-free. m owns this directory.
//!
//! Novel extensions (the `.kai` bundles whose manifest declares the `novel` capability, e.g. Toko) answer the host
//! routes `novel/chapters` (`getNovelChapters` / `debugNovelChapters`: rows `{ number, title, volume, sources: [{
//! provider, chapterKey, providerChapterId, language, translator, releaseDate }] }`, optionally wrapped in `{ results,
//! diagnostics }`) and `novel/content` (`getNovelContent`: `{ title, number, paragraphs: [{ index, text }] }`). This
//! module turns those bodies into typed rows ([`parse_chapter_rows`], [`parse_content`], [`parse_diagnostics`]),
//! cleans chapter HTML into paragraphs ([`paragraphs`]) and packs the four ids a chapter needs into one route-safe
//! string ([`ChapterKey`]). AniList novel metadata (`MANGA` type, `NOVEL` format) lives in [`anilist`]. Rows:
//! `store/tts.rs` (`novel_*` helpers over `chapter_lists`, `offline_chapters`, `read_progress`, `downloads`).

pub mod anilist;

use base64::Engine;
use scraper::{ElementRef, Html, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `SearchQueryVm::tags` entry that carries the country of origin filter of the novel search (`origin:KR`).
pub const ORIGIN_TAG_PREFIX: &str = "origin:";
/// Countries the novel search offers (AniList `countryOfOrigin`).
pub const ORIGINS: [&str; 4] = ["JP", "KR", "CN", "TW"];

/// Words of a chapter (whitespace-separated tokens; pure).
pub fn word_count(paragraphs: &[String]) -> u32 {
    paragraphs.iter().map(|p| p.split_whitespace().count() as u32).sum()
}

/// The country code of an `origin:XX` tag, upper-cased, when it is one of [`ORIGINS`].
pub fn origin_of(tags: &[String]) -> Option<String> {
    tags.iter()
        .filter_map(|t| t.strip_prefix(ORIGIN_TAG_PREFIX))
        .map(|c| c.trim().to_ascii_uppercase())
        .find(|c| ORIGINS.contains(&c.as_str()))
}

/// Elements whose text is never part of a chapter.
const SKIP: [&str; 12] = ["script", "style", "noscript", "iframe", "template", "svg", "button", "form", "nav", "ins", "select", "head"];
/// Elements that start / end a paragraph.
const BLOCK: [&str; 22] = [
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "blockquote", "section", "article", "tr", "hr", "pre", "ul", "ol",
    "table", "header", "footer", "main", "aside",
];

/// HTML of a chapter -> plain-text paragraphs: `<p>` / block elements and `<br><br>` split, a single `<br>` is a
/// space, headings stay as their own paragraph, scripts / styles / forms are dropped, entities decoded (by the HTML
/// parser), whitespace collapsed. Plain text (no tags) splits on blank lines.
pub fn paragraphs(html: &str) -> Vec<String> {
    let doc = Html::parse_fragment(html);
    let mut out = Vec::new();
    let mut cur = String::new();
    walk(doc.root_element(), &mut out, &mut cur);
    flush(&mut out, &mut cur);
    out
}

fn walk(el: ElementRef<'_>, out: &mut Vec<String>, cur: &mut String) {
    for child in el.children() {
        match child.value() {
            Node::Text(t) => cur.push_str(t),
            Node::Element(e) => {
                let name = e.name().to_ascii_lowercase();
                if SKIP.contains(&name.as_str()) {
                    continue;
                }
                if name == "br" {
                    cur.push('\n');
                    continue;
                }
                let block = BLOCK.contains(&name.as_str());
                if block {
                    flush(out, cur);
                }
                if let Some(inner) = ElementRef::wrap(child) {
                    walk(inner, out, cur);
                }
                if block {
                    flush(out, cur);
                }
            }
            _ => {}
        }
    }
}

/// Close the paragraph(s) collected so far: blank lines separate paragraphs, single newlines are spaces.
fn flush(out: &mut Vec<String>, cur: &mut String) {
    let text = std::mem::take(cur).replace("\r\n", "\n").replace('\u{a0}', " ");
    let mut para = String::new();
    for line in text.split('\n') {
        if line.trim().is_empty() {
            if !para.trim().is_empty() {
                push_clean(out, &para);
                para.clear();
            }
            continue;
        }
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(line);
    }
    push_clean(out, &para);
}

fn push_clean(out: &mut Vec<String>, text: &str) {
    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !clean.is_empty() {
        out.push(clean);
    }
}

/// Everything the reader needs to fetch one chapter again, packed into the opaque `ChapterRefVm::chapter_id` (the
/// `ext_id` half is the installed extension). Encoded as URL-safe base64 of a small JSON object so it survives the
/// `/novels/:id/read/:ext_id/:chapter_id` route (the router does not escape `/`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterKey {
    /// UI media id of the novel (AniList id as text).
    #[serde(rename = "m")]
    pub media_id: String,
    /// Sub-provider inside the extension (`novelfire`, `wuxiaworld`, ...).
    #[serde(rename = "p")]
    pub provider: String,
    /// `chapterKey` of the extension (`<provider>:<providerChapterId>`).
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "c", default, skip_serializing_if = "String::is_empty")]
    pub provider_chapter_id: String,
    #[serde(rename = "n", default)]
    pub number: f32,
}

impl ChapterKey {
    pub fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json)
    }

    pub fn decode(s: &str) -> Option<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s.trim().as_bytes()).ok()?;
        let k: Self = serde_json::from_slice(&bytes).ok()?;
        (!k.key.is_empty()).then_some(k)
    }
}

/// One chapter of one sub-provider of a novel extension (also the JSON cached in `chapter_lists.chapters_json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderChapter {
    pub provider: String,
    #[serde(default)]
    pub provider_name: Option<String>,
    pub chapter_key: String,
    #[serde(default)]
    pub provider_chapter_id: String,
    pub number: f32,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub volume: Option<u32>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub translator: Option<String>,
    #[serde(default)]
    pub published_unix: Option<i64>,
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn num_of(v: &Value) -> Option<f32> {
    match v {
        Value::Number(n) => n.as_f64().map(|f| f as f32),
        Value::String(s) => s.trim().parse::<f32>().ok(),
        _ => None,
    }
    .filter(|n| n.is_finite())
}

fn volume_of(v: &Value) -> Option<u32> {
    num_of(v).filter(|n| *n >= 0.0).map(|n| n as u32)
}

/// `2024-05-01`, `2024-05-01T10:00:00Z` or a unix time (seconds or milliseconds) -> unix seconds.
pub fn parse_release(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(if n > 100_000_000_000 { n / 1000 } else { n }).filter(|n| *n > 0);
    }
    let s = v.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(t) = time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339) {
        return Some(t.unix_timestamp());
    }
    let date = s.get(..10).unwrap_or(s);
    let mut parts = date.split('-');
    let y: i32 = parts.next()?.parse().ok()?;
    let m: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    let month = time::Month::try_from(m).ok()?;
    let day = time::Date::from_calendar_date(y, month, d).ok()?;
    Some(day.midnight().assume_utc().unix_timestamp())
}

/// The chapter rows of a `novel/chapters` answer: a bare array, `{ chapters }` or `{ results }` (the debug variant).
/// Rows with `sources` yield one entry per source; flat rows are one source themselves (`fallback_provider` when
/// they name none). Rows without a positive number are dropped.
pub fn parse_chapter_rows(body: &Value, fallback_provider: &str) -> Vec<ProviderChapter> {
    let list = body
        .as_array()
        .or_else(|| body["results"].as_array())
        .or_else(|| body["chapters"].as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for row in &list {
        let Some(number) = num_of(&row["number"]).filter(|n| *n > 0.0) else { continue };
        let title = str_of(&row["title"]);
        let volume = volume_of(&row["volume"]);
        let mut push = |s: &Value| {
            let provider = str_of(&s["provider"]).unwrap_or_else(|| fallback_provider.to_owned());
            let key = str_of(&s["chapterKey"]).unwrap_or_else(|| format!("{provider}:{number}"));
            out.push(ProviderChapter {
                provider_name: str_of(&s["providerName"]).or_else(|| str_of(&row["providerName"])),
                provider_chapter_id: str_of(&s["providerChapterId"]).unwrap_or_else(|| key.clone()),
                chapter_key: key,
                provider,
                number,
                title: title.clone(),
                volume,
                language: str_of(&s["language"]).map(|l| l.to_ascii_lowercase()),
                translator: str_of(&s["translator"]),
                published_unix: parse_release(&s["releaseDate"]),
            });
        };
        match row["sources"].as_array() {
            Some(sources) if !sources.is_empty() => sources.iter().for_each(&mut push),
            _ => push(row),
        }
    }
    out
}

/// Chapters grouped by sub-provider (first appearance order), each sorted by number with duplicates (same number
/// and translator) removed.
pub fn group_by_provider(rows: Vec<ProviderChapter>) -> Vec<(String, Vec<ProviderChapter>)> {
    let mut groups: Vec<(String, Vec<ProviderChapter>)> = Vec::new();
    for r in rows {
        match groups.iter_mut().find(|(p, _)| *p == r.provider) {
            Some((_, list)) => list.push(r),
            None => groups.push((r.provider.clone(), vec![r])),
        }
    }
    for (_, list) in &mut groups {
        list.sort_by(|a, b| a.number.total_cmp(&b.number));
        list.dedup_by(|a, b| a.number == b.number && a.translator == b.translator);
    }
    groups
}

/// Distinct translators across rows, in first-seen order.
pub fn translators(rows: &[ProviderChapter]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in rows.iter().filter_map(|r| r.translator.as_deref()) {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// Previous / next chapter of `current` among `rows` by number, preferring the same sub-provider and translator.
pub fn neighbours<'a>(rows: &'a [ProviderChapter], current: &ChapterKey) -> (Option<&'a ProviderChapter>, Option<&'a ProviderChapter>) {
    let same_translator = rows
        .iter()
        .find(|r| r.chapter_key == current.key && r.provider == current.provider)
        .and_then(|r| r.translator.clone());
    let rank = |r: &ProviderChapter| -> u8 {
        u8::from(r.provider != current.provider) * 2 + u8::from(r.translator != same_translator)
    };
    let pick = |want_next: bool| -> Option<&'a ProviderChapter> {
        let candidates = rows.iter().filter(|r| if want_next { r.number > current.number } else { r.number < current.number });
        let target = if want_next {
            candidates.clone().map(|r| r.number).min_by(f32::total_cmp)
        } else {
            candidates.clone().map(|r| r.number).max_by(f32::total_cmp)
        }?;
        candidates.filter(|r| r.number == target).min_by_key(|r| rank(r))
    };
    (pick(false), pick(true))
}

/// Other sources of the same chapter number (the classic "trying another translator" fallback), best first.
pub fn alternatives<'a>(rows: &'a [ProviderChapter], current: &ChapterKey) -> Vec<&'a ProviderChapter> {
    let mut v: Vec<&ProviderChapter> =
        rows.iter().filter(|r| r.number == current.number && r.chapter_key != current.key).collect();
    v.sort_by_key(|r| u8::from(r.provider != current.provider));
    v
}

/// `(title, paragraphs)` of a `novel/content` answer. Paragraph objects (`{ index, text }`) are ordered by index and
/// stripped of any inline HTML; `html` / `content` string bodies go through [`paragraphs`].
pub fn parse_content(body: &Value) -> (Option<String>, Vec<String>) {
    let title = str_of(&body["title"]).filter(|t| !t.chars().all(|c| c.is_ascii_digit()));
    let mut out: Vec<(i64, String)> = Vec::new();
    if let Some(list) = body["paragraphs"].as_array().or_else(|| body.as_array()) {
        for (i, p) in list.iter().enumerate() {
            let (index, text) = match p {
                Value::String(s) => (i as i64, s.clone()),
                _ => (p["index"].as_i64().unwrap_or(i as i64), p["text"].as_str().unwrap_or_default().to_owned()),
            };
            let cleaned = if text.contains('<') { paragraphs(&text).join(" ") } else { text.split_whitespace().collect::<Vec<_>>().join(" ") };
            if !cleaned.is_empty() {
                out.push((index, cleaned));
            }
        }
        out.sort_by_key(|(i, _)| *i);
        return (title, out.into_iter().map(|(_, t)| t).collect());
    }
    for field in ["html", "content", "text"] {
        if let Some(s) = body[field].as_str().filter(|s| !s.trim().is_empty()) {
            return (title, paragraphs(s));
        }
    }
    (title, Vec::new())
}

/// Per sub-provider chapter counts of a `novel/chapters` answer (`novel.details.check-sources`): the `diagnostics`
/// of the debug route when present (`{ provider, status, resultCount }`), else counted from the rows.
pub fn parse_diagnostics(body: &Value, rows: &[ProviderChapter]) -> Vec<(String, u32)> {
    if let Some(list) = body["diagnostics"].as_array().filter(|l| !l.is_empty()) {
        let mut out: Vec<(String, u32)> = Vec::new();
        for d in list {
            let Some(name) = str_of(&d["provider"]) else { continue };
            let n = rows.iter().filter(|r| r.provider == name).count() as u32;
            let n = if n > 0 { n } else { d["resultCount"].as_u64().unwrap_or(0) as u32 };
            match out.iter_mut().find(|(p, _)| *p == name) {
                Some(slot) => slot.1 = slot.1.max(n),
                None => out.push((name, n)),
            }
        }
        return out;
    }
    group_by_provider(rows.to_vec()).into_iter().map(|(p, list)| (p, list.len() as u32)).collect()
}

/// Whether a manifest (`capabilities`, `apiServer.routes`) advertises the novel routes.
pub fn is_novel_manifest(capabilities: &[String], routes: &[String]) -> bool {
    capabilities.iter().any(|c| c == "novel") || routes.iter().any(|r| r == "novel/chapters" || r == "novel/content")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_words() {
        assert_eq!(word_count(&["one two".into(), " three ".into()]), 3);
    }

    #[test]
    fn html_becomes_paragraphs() {
        let html = "<div><h2>Chapter 1</h2><p>Hello&nbsp;<b>world</b>.</p><script>ads()</script>\
                    <p>Second\n   line</p>First<br>still first<br><br>Third &amp; last</div>";
        assert_eq!(paragraphs(html), vec!["Chapter 1", "Hello world.", "Second line", "First still first", "Third & last"]);
    }

    #[test]
    fn plain_text_splits_on_blank_lines() {
        assert_eq!(paragraphs("One\nline\n\nTwo"), vec!["One line", "Two"]);
        assert!(paragraphs("  <style>p{}</style> ").is_empty());
    }

    #[test]
    fn chapter_key_round_trips_and_is_route_safe() {
        let k = ChapterKey { media_id: "108592".into(), provider: "novelfire".into(), key: "novelfire:abc/ch-1?x=1".into(), provider_chapter_id: "abc/ch-1".into(), number: 1.5 };
        let s = k.encode();
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        assert_eq!(ChapterKey::decode(&s), Some(k));
        assert_eq!(ChapterKey::decode("not base64!"), None);
    }

    #[test]
    fn rows_with_sources_and_flat_rows() {
        let body = json!({ "results": [
            { "number": 1, "title": "Start", "volume": "1", "sources": [
                { "provider": "novelfire", "chapterKey": "novelfire:1", "providerChapterId": "1", "language": "EN", "translator": "TL A", "releaseDate": "2024-05-01" },
                { "provider": "wuxia", "chapterKey": "wuxia:1", "providerChapterId": "w1", "language": "en", "translator": null }
            ]},
            { "number": "2", "provider": "novelfire", "chapterKey": "novelfire:2" },
            { "number": 0, "chapterKey": "x" }
        ]});
        let rows = parse_chapter_rows(&body, "toko");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].language.as_deref(), Some("en"));
        assert_eq!(rows[0].volume, Some(1));
        assert_eq!(rows[0].published_unix, Some(1_714_521_600));
        assert_eq!(rows[2].number, 2.0);
        assert_eq!(rows[2].provider_chapter_id, "novelfire:2");
        let groups = group_by_provider(rows.clone());
        assert_eq!(groups.iter().map(|(p, l)| (p.as_str(), l.len())).collect::<Vec<_>>(), vec![("novelfire", 2), ("wuxia", 1)]);
        assert_eq!(translators(&rows), vec!["TL A".to_owned()]);
        assert_eq!(parse_chapter_rows(&json!([{ "number": 3, "chapterKey": "k" }]), "toko")[0].provider, "toko");
    }

    #[test]
    fn neighbours_prefer_the_same_provider() {
        let row = |p: &str, n: f32| ProviderChapter {
            provider: p.into(),
            provider_name: None,
            chapter_key: format!("{p}:{n}"),
            provider_chapter_id: String::new(),
            number: n,
            title: None,
            volume: None,
            language: None,
            translator: None,
            published_unix: None,
        };
        let rows = vec![row("a", 1.0), row("b", 2.0), row("a", 2.0), row("a", 3.0), row("b", 3.0)];
        let cur = ChapterKey { media_id: "1".into(), provider: "a".into(), key: "a:2".into(), provider_chapter_id: String::new(), number: 2.0 };
        let (prev, next) = neighbours(&rows, &cur);
        assert_eq!(prev.map(|r| r.chapter_key.as_str()), Some("a:1"));
        assert_eq!(next.map(|r| r.chapter_key.as_str()), Some("a:3"));
        assert_eq!(alternatives(&rows, &cur).iter().map(|r| r.chapter_key.as_str()).collect::<Vec<_>>(), vec!["b:2"]);
        let last = ChapterKey { number: 3.0, key: "b:3".into(), provider: "b".into(), ..cur };
        assert_eq!(neighbours(&rows, &last).1, None);
    }

    #[test]
    fn content_bodies() {
        let (title, p) = parse_content(&json!({ "title": "Ch 1", "paragraphs": [{ "index": 1, "text": "Two" }, { "index": 0, "text": "<i>One</i>  x" }, { "index": 2, "text": "  " }] }));
        assert_eq!(title.as_deref(), Some("Ch 1"));
        assert_eq!(p, vec!["One x", "Two"]);
        assert_eq!(parse_content(&json!({ "html": "<p>a</p><p>b</p>" })).1, vec!["a", "b"]);
        assert!(parse_content(&json!({})).1.is_empty());
    }

    #[test]
    fn diagnostics_and_origin() {
        let rows = parse_chapter_rows(&json!([{ "number": 1, "provider": "a", "chapterKey": "a:1" }]), "x");
        let d = parse_diagnostics(&json!({ "diagnostics": [{ "provider": "a", "resultCount": 9 }, { "provider": "b", "resultCount": 0 }] }), &rows);
        assert_eq!(d, vec![("a".to_owned(), 1), ("b".to_owned(), 0)]);
        assert_eq!(parse_diagnostics(&json!([]), &rows), vec![("a".to_owned(), 1)]);
        assert_eq!(origin_of(&["action".into(), "origin:kr".into()]).as_deref(), Some("KR"));
        assert_eq!(origin_of(&["origin:XX".into()]), None);
        assert!(is_novel_manifest(&["novel".into()], &[]));
        assert!(!is_novel_manifest(&["chapters".into()], &["manga/chapters".into()]));
    }

    #[test]
    fn release_dates() {
        assert_eq!(parse_release(&json!("2024-05-01T00:00:00Z")), Some(1_714_521_600));
        assert_eq!(parse_release(&json!(1_714_521_600_000_i64)), Some(1_714_521_600));
        assert_eq!(parse_release(&json!("soon")), None);
    }
}
