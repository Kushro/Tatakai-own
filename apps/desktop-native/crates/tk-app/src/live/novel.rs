//! Novels + narration (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist
//! `novel.*`; classic `pages/novel/*`, `core/content/novel-client.ts`, `components/novel/*`).
//!
//! - Catalogue: AniList `MANGA` with `format: NOVEL` through `tk_core::novel::anilist` (m's own small client so
//!   it does not wait on worker l's manga queries): home rows (`novel.feed.<id>`), filtered search (genre, status,
//!   origin `origin:XX` tag, order), details with authors.
//! - Chapters and text: the installed extensions whose manifest declares the `novel` capability (or the
//!   `novel/chapters` route), through `ExtensionHost::call(ext, "novel/chapters" | "novel/content", ..)`; bodies
//!   parsed by `tk_core::novel`. One `ChapterProviderVm` per sub-provider of the extension (`novelfire`, ...).
//!   Lists are cached in `chapter_lists` (`kind = 'novel'`), downloaded text in `offline_chapters.text_json`.
//!   `ChapterRefVm::chapter_id` is `tk_core::novel::ChapterKey::encode()` (route safe).
//! - Reading positions are written by the UI through the shared `set_read_progress(ReadKindVm::Novel, ..)` of
//!   worker l; this file reads them back from `read_progress` for "Continue reading" and the details resume.
//! - Downloads (`novel.download.chapters`): the chapter text is fetched here (it needs the extension host and the
//!   novel parsers) and stored offline; each chapter is recorded as a finished `downloads` row with `kind =
//!   'novel'` so worker h's Downloads page lists it (contract section 0.10 routes novel chapters through h's queue;
//!   text chapters are small, so they complete synchronously and enter the table already `completed` / `failed`).
//! - Narration: `crate::tts` (`b.tts`), keys in `b.secrets` under `keys::tts(provider.id())`, clips cached in the
//!   `tts_cache` table.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use tk_core::novel::anilist::{AniListNovels, NovelFeed, NovelInfo, NovelSearch};
use tk_core::novel::{self as core_novel, ChapterKey, ProviderChapter};
use tk_core::secrets::keys;
use tk_core::store::tts::{NovelDownloadRow, NovelProgressRow, TtsClipRow};
use tk_ui::backend::{
    Backend, ChapterProviderVm, ChapterRefVm, ChapterVm, ContinueReadVm, KittenStatusVm, ListStatusVm,
    MediaVm, NovelContentVm, NovelDetailsVm, ReadHomeVm, ReadProgressVm, SearchPageVm, SearchQueryVm,
    SectionVm, TtsClipVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm, UiResult,
};
use tk_ui::model::novel::{KITTEN_DEFAULT_MODEL, clip_url, kitten_model_info};
use tk_ui::model::{ErrorKind, ReadKindVm, TtsProviderVm, UiError};

use super::{LiveBackend, provider_err, to_vm};
use crate::settings_store;

/// Cached chapter lists are reused for this long unless the user refreshes.
const LIST_TTL_SECS: i64 = 6 * 3600;
const CHAPTERS_TIMEOUT: Duration = Duration::from_secs(45);
const CHECK_TIMEOUT: Duration = Duration::from_secs(120);
const CONTENT_TIMEOUT: Duration = Duration::from_secs(40);
/// Narration clips kept on disk (oldest dropped first).
const TTS_CACHE_MAX_BYTES: i64 = 256 * 1024 * 1024;
/// "Continue reading" cards on the Novels home.
const CONTINUE_LIMIT: u32 = 12;

fn other(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn ext_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Extension, e.to_string())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Novels seen this session, by UI id (AniList id as text).
fn info_cache() -> &'static Mutex<HashMap<String, NovelInfo>> {
    static CACHE: OnceLock<Mutex<HashMap<String, NovelInfo>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember(infos: &[NovelInfo]) {
    if let Ok(mut c) = info_cache().lock() {
        for i in infos {
            if let Some(id) = i.media.media_ref.anilist_id {
                c.insert(id.to_string(), i.clone());
            }
        }
    }
}

fn anilist(b: &LiveBackend) -> AniListNovels {
    AniListNovels::new(b.tts.client())
}

/// AniList id of a UI novel id (`"85737"` or `"anilist:85737"`).
pub fn anilist_id(id: &str) -> UiResult<u64> {
    let raw = id.trim();
    raw.strip_prefix("anilist:")
        .unwrap_or(raw)
        .parse::<u64>()
        .map_err(|_| other(format!("invalid novel id {id}")))
}

/// Poster card of a novel: the shared mapping (title language) with a novel subtitle (`2010 - 16 vol`).
pub fn novel_vm(info: &NovelInfo) -> MediaVm {
    let mut vm = to_vm(&info.media);
    let mut parts = Vec::new();
    if let Some(y) = info.media.year {
        parts.push(y.to_string());
    }
    if let Some(v) = info.volumes {
        parts.push(format!("{v} vol"));
    } else if let Some(c) = info.chapters {
        parts.push(format!("{c} ch"));
    }
    vm.subtitle = parts.join(" - ");
    vm
}

async fn info(b: &LiveBackend, id: &str) -> UiResult<NovelInfo> {
    if let Some(i) = info_cache().lock().ok().and_then(|c| c.get(id).cloned())
        && !i.authors.is_empty()
    {
        return Ok(i);
    }
    let n = anilist_id(id)?;
    let found = anilist(b).details(n).await.map_err(provider_err)?;
    let i = found.ok_or_else(|| other(format!("novel {id} was not found")))?;
    remember(std::slice::from_ref(&i));
    b.remember_title(id, &i.media.title, i.media.cover.as_deref());
    Ok(i)
}

fn progress_vm(r: &NovelProgressRow) -> ReadProgressVm {
    ReadProgressVm {
        chapter: ChapterRefVm {
            ext_id: r.ext_id.clone(),
            chapter_id: r.chapter_id.clone(),
        },
        chapter_number: r.chapter_number,
        position: r.position,
        total: r.total,
        completed: r.completed,
        updated_unix: r.updated_at,
    }
}

// ---- home / search / details --------------------------------------------------------------------------

pub async fn home(b: &LiveBackend) -> UiResult<ReadHomeVm> {
    let al = anilist(b);
    let (trending, popular, top, releasing) = tokio::join!(
        al.feed(NovelFeed::Trending, 1),
        al.feed(NovelFeed::Popular, 1),
        al.feed(NovelFeed::TopRated, 1),
        al.feed(NovelFeed::Releasing, 1),
    );
    let mut sections = Vec::new();
    for (feed, res) in [
        (NovelFeed::Trending, trending),
        (NovelFeed::Popular, popular),
        (NovelFeed::TopRated, top),
        (NovelFeed::Releasing, releasing),
    ] {
        let items = res
            .map(|page| {
                remember(&page.items);
                page.items.iter().map(novel_vm).collect::<Vec<_>>()
            })
            .map_err(provider_err);
        sections.push(SectionVm {
            title_key: format!("novel.feed.{}", feed.id()),
            items,
        });
    }
    // Updates of the novels the user follows (AniList manga list mirror, status "reading").
    match b
        .read_list_entries(ReadKindVm::Novel, Some(ListStatusVm::Watching))
        .await
    {
        Ok(entries) if entries.is_empty() => {}
        Ok(entries) => {
            let ids: Vec<u64> = entries
                .iter()
                .filter_map(|e| anilist_id(&e.media.id).ok())
                .take(50)
                .collect();
            let items = match al.by_ids(&ids).await {
                Ok(list) => {
                    remember(&list);
                    let mut list = list;
                    // Releasing first: those are the ones with new chapters.
                    list.sort_by_key(|i| u8::from(i.media.status.as_deref() != Some("RELEASING")));
                    Ok(list.iter().map(novel_vm).collect())
                }
                Err(e) => Err(provider_err(e)),
            };
            sections.insert(
                0,
                SectionVm {
                    title_key: "novel.feed.following".into(),
                    items,
                },
            );
        }
        Err(e) => sections.push(SectionVm {
            title_key: "novel.feed.following".into(),
            items: Err(e),
        }),
    }
    Ok(ReadHomeVm {
        continue_reading: continue_reading(b, &al).await,
        sections,
    })
}

async fn continue_reading(b: &LiveBackend, al: &AniListNovels) -> Vec<ContinueReadVm> {
    let rows = b.store.novel_recent_progress(CONTINUE_LIMIT).unwrap_or_default();
    let missing: Vec<u64> = rows
        .iter()
        .filter(|r| {
            r.title.is_none()
                && info_cache()
                    .lock()
                    .ok()
                    .is_none_or(|c| !c.contains_key(&r.media_id))
        })
        .filter_map(|r| anilist_id(&r.media_id).ok())
        .collect();
    if !missing.is_empty()
        && let Ok(list) = al.by_ids(&missing).await
    {
        remember(&list);
    }
    rows.iter()
        .filter_map(|r| {
            let media = match info_cache()
                .lock()
                .ok()
                .and_then(|c| c.get(&r.media_id).map(novel_vm))
            {
                Some(vm) => vm,
                None => MediaVm {
                    id: r.media_id.clone(),
                    title: r.title.clone()?,
                    cover: r.poster.clone(),
                    color: None,
                    subtitle: String::new(),
                    score: None,
                },
            };
            Some(ContinueReadVm {
                media,
                kind: ReadKindVm::Novel,
                progress: progress_vm(r),
            })
        })
        .collect()
}

/// `SearchQueryVm` -> AniList novel search variables (`origin:XX` tag = country of origin).
pub fn search_of(q: &SearchQueryVm) -> NovelSearch {
    NovelSearch {
        query: q.query.clone(),
        genres: q.genres.clone(),
        status: q.status.clone(),
        origin: core_novel::origin_of(&q.tags),
        sort: q.sort.id().to_owned(),
        adult: q.adult,
        page: q.page.max(1),
    }
}

pub async fn search(b: &LiveBackend, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    let page = anilist(b).search(&search_of(q)).await.map_err(provider_err)?;
    remember(&page.items);
    Ok(SearchPageVm {
        items: page.items.iter().map(novel_vm).collect(),
        page: page.page,
        has_next: page.has_next,
        total: page.total,
    })
}

/// Every cached chapter row of a novel (all extensions).
fn cached_rows(b: &LiveBackend, media_id: &str) -> Vec<(String, Vec<ProviderChapter>)> {
    b.store
        .novel_chapter_lists(media_id)
        .unwrap_or_default()
        .into_iter()
        .map(|(ext, json)| {
            (
                ext,
                serde_json::from_str::<Vec<ProviderChapter>>(&json).unwrap_or_default(),
            )
        })
        .collect()
}

pub async fn details(b: &LiveBackend, id: &str) -> UiResult<NovelDetailsVm> {
    let i = info(b, id).await?;
    let all: Vec<ProviderChapter> = cached_rows(b, id)
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .collect();
    let resume = b
        .store
        .novel_progress_rows(id)
        .ok()
        .and_then(|rows| rows.first().map(progress_vm));
    Ok(NovelDetailsVm {
        media: novel_vm(&i),
        banner: i.media.banner.clone(),
        synopsis: i.media.synopsis.clone(),
        genres: i.media.genres.clone(),
        status: i.media.status.clone(),
        chapters_total: i.chapters,
        volumes_total: i.volumes,
        authors: i.authors.clone(),
        translators: core_novel::translators(&all),
        adult: i.adult,
        resume,
    })
}

// ---- extensions -------------------------------------------------------------------------------------

/// An enabled installed extension that serves novels.
#[derive(Debug, Clone)]
struct NovelExt {
    id: String,
    name: String,
}

fn novel_exts(b: &LiveBackend) -> Vec<NovelExt> {
    let host = b.ext.host();
    let root = host.root().to_path_buf();
    b.ext
        .installed()
        .into_iter()
        .filter(|e| e.enabled && e.compat != tk_core::Compat::Unsupported)
        .filter(|e| {
            let Ok(text) = std::fs::read_to_string(root.join(&e.id).join("manifest.json")) else {
                return false;
            };
            let Ok(m) = tk_ext::Manifest::parse(&text) else {
                return false;
            };
            let routes = m
                .api_server
                .as_ref()
                .and_then(|a| a.routes.clone())
                .unwrap_or_default();
            core_novel::is_novel_manifest(&m.capabilities, &routes)
        })
        .map(|e| NovelExt {
            id: e.id,
            name: e.name,
        })
        .collect()
}

/// `novel/chapters` parameters of the classic host (`NovelChapterParams` of the Toko bundle).
pub fn chapter_params(info: &NovelInfo, full_scan: bool) -> Value {
    json!({
        "anilistId": info.media.media_ref.anilist_id,
        "malId": info.media.media_ref.mal_id,
        "title": info.titles.first().cloned().unwrap_or_else(|| info.media.title.clone()),
        "titles": info.titles,
        "fullScan": full_scan,
    })
}

async fn call_ext(
    b: &LiveBackend,
    ext_id: &str,
    route: &str,
    params: Value,
    limit: Duration,
) -> UiResult<Value> {
    let host = b.ext.host();
    match tokio::time::timeout(limit, host.call(ext_id, route, params)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(ext_err(e)),
        Err(_) => Err(UiError::new(
            ErrorKind::Extension,
            format!("{ext_id} did not answer {route} in {}s", limit.as_secs()),
        )),
    }
}

/// Chapter rows of one extension: the fresh cache, else the extension (cached afterwards), else a stale cache.
async fn ext_rows(
    b: &LiveBackend,
    info: &NovelInfo,
    media_id: &str,
    ext: &NovelExt,
    refresh: bool,
) -> UiResult<Vec<ProviderChapter>> {
    let now = now_unix();
    if !refresh
        && let Ok(Some(json)) = b.store.novel_chapter_list(media_id, &ext.id, LIST_TTL_SECS, now)
        && let Ok(rows) = serde_json::from_str::<Vec<ProviderChapter>>(&json)
    {
        return Ok(rows);
    }
    match call_ext(
        b,
        &ext.id,
        "novel/chapters",
        chapter_params(info, false),
        CHAPTERS_TIMEOUT,
    )
    .await
    {
        Ok(body) => {
            let rows = core_novel::parse_chapter_rows(&body, &ext.id);
            if let Ok(json) = serde_json::to_string(&rows) {
                let _ = b.store.set_novel_chapter_list(media_id, &ext.id, &json, now);
            }
            Ok(rows)
        }
        Err(e) => {
            let stale = cached_rows(b, media_id)
                .into_iter()
                .find(|(x, _)| *x == ext.id)
                .map(|(_, r)| r);
            match stale {
                Some(rows) if !rows.is_empty() => Ok(rows),
                _ => Err(e),
            }
        }
    }
}

/// Read fraction of a chapter from the stored positions.
fn read_fraction(progress: &[NovelProgressRow], chapter_id: &str) -> f64 {
    progress
        .iter()
        .find(|p| p.chapter_id == chapter_id)
        .map_or(0.0, |p| {
            if p.completed {
                1.0
            } else {
                p.total
                    .filter(|t| *t > 0)
                    .map_or(0.0, |t| (f64::from(p.position) / f64::from(t)).clamp(0.0, 1.0))
            }
        })
}

pub fn chapter_key(media_id: &str, row: &ProviderChapter) -> ChapterKey {
    ChapterKey {
        media_id: media_id.to_owned(),
        provider: row.provider.clone(),
        key: row.chapter_key.clone(),
        provider_chapter_id: row.provider_chapter_id.clone(),
        number: row.number,
    }
}

/// Group rows per sub-provider, optionally keeping one translator, into the UI rows.
fn provider_vms(
    media_id: &str,
    ext: &NovelExt,
    rows: Vec<ProviderChapter>,
    translator: Option<&str>,
    many_exts: bool,
    offline: &HashSet<(String, String)>,
    progress: &[NovelProgressRow],
) -> Vec<ChapterProviderVm> {
    core_novel::group_by_provider(rows)
        .into_iter()
        .filter_map(|(provider, list)| {
            let list: Vec<ProviderChapter> = list
                .into_iter()
                .filter(|r| {
                    translator
                        .is_none_or(|t| r.translator.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(t)))
                })
                .collect();
            if list.is_empty() {
                return None;
            }
            let display = list
                .iter()
                .find_map(|r| r.provider_name.clone())
                .unwrap_or(provider);
            let lang = list.iter().find_map(|r| r.language.clone());
            let chapters = list
                .iter()
                .map(|r| {
                    let chapter_id = chapter_key(media_id, r).encode();
                    ChapterVm {
                        read: read_fraction(progress, &chapter_id),
                        downloaded: offline.contains(&(ext.id.clone(), chapter_id.clone())),
                        reference: ChapterRefVm {
                            ext_id: ext.id.clone(),
                            chapter_id,
                        },
                        number: r.number,
                        volume: r.volume,
                        title: r.title.clone(),
                        lang: r.language.clone(),
                        group: r.translator.clone(),
                        published_unix: r.published_unix,
                        pages: None,
                    }
                })
                .collect();
            let ext_name = if many_exts {
                format!("{display} · {}", ext.name)
            } else {
                display
            };
            Some(ChapterProviderVm {
                ext_id: ext.id.clone(),
                ext_name,
                lang,
                chapters,
                error: None,
            })
        })
        .collect()
}

pub async fn chapters(
    b: &LiveBackend,
    id: &str,
    translator: Option<&str>,
    refresh: bool,
) -> UiResult<Vec<ChapterProviderVm>> {
    let exts = novel_exts(b);
    if exts.is_empty() {
        return Ok(Vec::new());
    }
    let i = info(b, id).await?;
    let offline: HashSet<(String, String)> = b
        .store
        .novel_offline_ids(id)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let progress = b.store.novel_progress_rows(id).unwrap_or_default();
    let translator = translator.map(str::trim).filter(|t| !t.is_empty());
    let many = exts.len() > 1;
    let mut out = Vec::new();
    for ext in &exts {
        match ext_rows(b, &i, id, ext, refresh).await {
            Ok(rows) => out.extend(provider_vms(id, ext, rows, translator, many, &offline, &progress)),
            Err(e) => out.push(ChapterProviderVm {
                ext_id: ext.id.clone(),
                ext_name: ext.name.clone(),
                lang: None,
                chapters: Vec::new(),
                error: Some(e),
            }),
        }
    }
    Ok(out)
}

/// `{ title, paragraphs }` JSON of an offline chapter.
pub fn offline_json(title: &str, paragraphs: &[String]) -> String {
    json!({ "title": title, "paragraphs": paragraphs }).to_string()
}

pub fn parse_offline(text: &str) -> Option<(String, Vec<String>)> {
    let v: Value = serde_json::from_str(text).ok()?;
    let paragraphs: Vec<String> = v["paragraphs"]
        .as_array()?
        .iter()
        .filter_map(|p| p.as_str().map(str::to_owned))
        .filter(|p| !p.trim().is_empty())
        .collect();
    (!paragraphs.is_empty()).then(|| (v["title"].as_str().unwrap_or_default().to_owned(), paragraphs))
}

/// Text of a chapter from its extension, trying the other sources of the same number when it fails.
async fn fetch_text(b: &LiveBackend, ext_id: &str, key: &ChapterKey) -> UiResult<(String, Vec<String>)> {
    let attempt = |k: ChapterKey| async move {
        let params = json!({
            "chapterKey": k.key,
            "provider": k.provider,
            "providerChapterId": k.provider_chapter_id,
            "anilistId": anilist_id(&k.media_id).ok(),
        });
        let body = call_ext(b, ext_id, "novel/content", params, CONTENT_TIMEOUT).await?;
        let (title, paragraphs) = core_novel::parse_content(&body);
        if paragraphs.is_empty() {
            return Err(UiError::new(ErrorKind::NoSource, "the chapter has no text"));
        }
        Ok::<(String, Vec<String>), UiError>((title.unwrap_or_default(), paragraphs))
    };
    let first = match attempt(key.clone()).await {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    let rows: Vec<ProviderChapter> = cached_rows(b, &key.media_id)
        .into_iter()
        .filter(|(x, _)| x == ext_id)
        .flat_map(|(_, r)| r)
        .collect();
    for alt in core_novel::alternatives(&rows, key).into_iter().take(3) {
        if let Ok(v) = attempt(chapter_key(&key.media_id, alt)).await {
            return Ok(v);
        }
    }
    Err(first)
}

pub async fn content(b: &LiveBackend, chapter: &ChapterRefVm) -> UiResult<NovelContentVm> {
    let key = ChapterKey::decode(&chapter.chapter_id).ok_or_else(|| other("invalid chapter id"))?;
    let offline_text = b
        .store
        .novel_offline_text(&key.media_id, &chapter.ext_id, &chapter.chapter_id)
        .ok()
        .flatten();
    let (title, paragraphs, offline) = match offline_text.as_deref().and_then(parse_offline) {
        Some((t, p)) => (t, p, true),
        None => {
            let (t, p) = fetch_text(b, &chapter.ext_id, &key).await?;
            (t, p, false)
        }
    };
    let rows: Vec<ProviderChapter> = cached_rows(b, &key.media_id)
        .into_iter()
        .filter(|(x, _)| *x == chapter.ext_id)
        .flat_map(|(_, r)| r)
        .collect();
    let (prev, next) = core_novel::neighbours(&rows, &key);
    let to_ref = |r: &ProviderChapter| ChapterRefVm {
        ext_id: chapter.ext_id.clone(),
        chapter_id: chapter_key(&key.media_id, r).encode(),
    };
    let row_title = rows
        .iter()
        .find(|r| r.chapter_key == key.key)
        .and_then(|r| r.title.clone());
    Ok(NovelContentVm {
        reference: chapter.clone(),
        title: if title.trim().is_empty() {
            row_title.unwrap_or_default()
        } else {
            title
        },
        word_count: core_novel::word_count(&paragraphs),
        paragraphs,
        prev: prev.map(to_ref),
        next: next.map(to_ref),
        offline,
    })
}

pub async fn check_sources(b: &LiveBackend, id: &str) -> UiResult<Vec<(String, u32)>> {
    let exts = novel_exts(b);
    if exts.is_empty() {
        return Ok(Vec::new());
    }
    let i = info(b, id).await?;
    let mut out: Vec<(String, u32)> = Vec::new();
    let mut first_err = None;
    for ext in &exts {
        match call_ext(
            b,
            &ext.id,
            "novel/chapters",
            chapter_params(&i, true),
            CHECK_TIMEOUT,
        )
        .await
        {
            Ok(body) => {
                let rows = core_novel::parse_chapter_rows(&body, &ext.id);
                if let Ok(json) = serde_json::to_string(&rows) {
                    let _ = b.store.set_novel_chapter_list(id, &ext.id, &json, now_unix());
                }
                let counts = core_novel::parse_diagnostics(&body, &rows);
                if counts.is_empty() {
                    out.push((ext.name.clone(), rows.len() as u32));
                } else {
                    out.extend(counts);
                }
            }
            Err(e) => {
                out.push((ext.name.clone(), 0));
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if out.iter().all(|(_, n)| *n == 0) => Err(e),
        _ => Ok(out),
    }
}

/// Download label of a chapter (`Ch. 12 · Title`).
pub fn chapter_label(number: f32, title: &str) -> String {
    let n = tk_ui::model::novel::chapter_label(number);
    if title.trim().is_empty() {
        format!("Ch. {n}")
    } else {
        format!("Ch. {n} · {}", title.trim())
    }
}

pub async fn download_chapters(b: &LiveBackend, id: &str, chapters: &[ChapterRefVm]) -> UiResult<u32> {
    if chapters.is_empty() {
        return Ok(0);
    }
    let i = info(b, id).await.ok();
    let (title, poster) = match &i {
        Some(i) => (i.media.title.clone(), i.media.cover.clone()),
        None => (id.to_owned(), None),
    };
    let mut saved = 0;
    let mut first_err = None;
    for c in chapters {
        let Some(key) = ChapterKey::decode(&c.chapter_id) else {
            continue;
        };
        let now = now_unix();
        let res = fetch_text(b, &c.ext_id, &key).await;
        let (state, bytes, error, label) = match res {
            Ok((t, paragraphs)) => {
                let text = offline_json(&t, &paragraphs);
                match b
                    .store
                    .put_novel_offline(&key.media_id, &c.ext_id, &c.chapter_id, &text, now)
                {
                    Ok(()) => {
                        saved += 1;
                        (
                            "completed",
                            text.len() as i64,
                            None,
                            chapter_label(key.number, &t),
                        )
                    }
                    Err(e) => ("failed", 0, Some(e.to_string()), chapter_label(key.number, &t)),
                }
            }
            Err(e) => {
                let detail = e.detail.clone();
                first_err.get_or_insert(e);
                ("failed", 0, Some(detail), chapter_label(key.number, ""))
            }
        };
        let row = NovelDownloadRow {
            media_id: id.to_owned(),
            title: title.clone(),
            poster: poster.clone(),
            chapter: label,
            url: format!("novel:{}/{}", c.ext_id, c.chapter_id),
            state: state.to_owned(),
            bytes,
            error,
            at: now,
        };
        if let Err(e) = b.store.novel_insert_download(&row) {
            tracing::warn!(error = %e, "could not record the novel chapter download");
        }
    }
    match first_err {
        Some(e) if saved == 0 => Err(e),
        _ => Ok(saved),
    }
}

// ---- narration --------------------------------------------------------------------------------------

async fn secret(b: &LiveBackend, provider: TtsProviderVm) -> UiResult<Option<String>> {
    let secrets = b.secrets.clone();
    let key = keys::tts(provider.id());
    let v = tokio::task::spawn_blocking(move || secrets.get(&key))
        .await
        .map_err(other)?
        .map_err(other)?;
    Ok(v.filter(|k| !k.trim().is_empty()))
}

/// Kitten model id of the saved prefs.
fn kitten_model(prefs: &tk_ui::model::TtsPrefs) -> String {
    prefs
        .model
        .as_deref()
        .filter(|m| kitten_model_info(m).is_some())
        .unwrap_or(KITTEN_DEFAULT_MODEL)
        .to_owned()
}

pub async fn tts_voices(b: &LiveBackend, provider: TtsProviderVm) -> UiResult<Vec<TtsVoiceVm>> {
    let prefs = settings_store::load(&b.settings_file).novel.tts;
    let key = if provider.needs_key() {
        secret(b, provider).await?
    } else {
        None
    };
    if provider == TtsProviderVm::OpenAiCompatible
        && let Some(ep) = prefs.endpoint.as_deref()
    {
        return Ok(b.tts.compatible_voices(ep, key.as_deref()).await);
    }
    b.tts.voices(provider, key.as_deref()).await
}

fn clip_from_row(row: &TtsClipRow) -> Option<TtsClipVm> {
    let path = Path::new(&row.path);
    if !path.is_file() {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(TtsClipVm {
        path: path.to_path_buf(),
        url: clip_url(&name),
        mime: row.mime.clone(),
        duration_ms: row.duration_ms,
    })
}

pub async fn tts_speak(b: &LiveBackend, req: &TtsRequestVm) -> UiResult<TtsSpeakVm> {
    let prefs = settings_store::load(&b.settings_file).novel.tts;
    b.tts.set_pitch(prefs.pitch);
    let voice = req
        .voice
        .clone()
        .or_else(|| Some(prefs.voice.clone()))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    let req = TtsRequestVm { voice, ..req.clone() };
    let model = match req.provider {
        TtsProviderVm::Kitten => Some(kitten_model(&prefs)),
        TtsProviderVm::OpenAi | TtsProviderVm::OpenAiCompatible | TtsProviderVm::ElevenLabs => {
            prefs.model.clone()
        }
        TtsProviderVm::System => None,
    };
    let endpoint = (req.provider == TtsProviderVm::OpenAiCompatible)
        .then(|| prefs.endpoint.clone())
        .flatten();
    let key = b.tts.clip_key(&req, model.as_deref(), endpoint.as_deref());
    if let Ok(Some(row)) = b.store.tts_clip(&key) {
        match clip_from_row(&row) {
            Some(clip) => {
                return Ok(TtsSpeakVm {
                    clip: Some(clip),
                    native: false,
                });
            }
            None => {
                let _ = b.store.delete_tts_clip(&key);
            }
        }
    }
    let api_key = if req.provider.needs_key() {
        secret(b, req.provider).await?
    } else {
        None
    };
    let vm = b
        .tts
        .speak(&req, api_key.as_deref(), endpoint.as_deref(), model.as_deref())
        .await?;
    if let Some(c) = &vm.clip {
        let bytes = std::fs::metadata(&c.path).map(|m| m.len() as i64).unwrap_or(0);
        let row = TtsClipRow {
            key,
            provider: req.provider.id().to_owned(),
            voice: req.voice.clone().unwrap_or_default(),
            rate: u32::from(req.rate),
            path: c.path.to_string_lossy().into_owned(),
            mime: c.mime.clone(),
            duration_ms: c.duration_ms,
            bytes,
            created_at: now_unix(),
        };
        if b.store.put_tts_clip(&row).is_ok()
            && let Ok(paths) = b.store.evict_tts_cache(TTS_CACHE_MAX_BYTES)
        {
            for p in paths {
                let _ = std::fs::remove_file(p);
            }
        }
    }
    Ok(vm)
}

pub async fn tts_stop(b: &LiveBackend) -> UiResult<()> {
    b.tts.stop().await
}

pub async fn set_tts_key(b: &LiveBackend, provider: TtsProviderVm, key: Option<&str>) -> UiResult<()> {
    if !provider.needs_key() {
        return Err(other("this narration provider takes no key"));
    }
    let name = keys::tts(provider.id());
    let value = key.map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned);
    let secrets = b.secrets.clone();
    tokio::task::spawn_blocking(move || match value {
        Some(v) => secrets.set(&name, &v),
        None => secrets.delete(&name),
    })
    .await
    .map_err(other)?
    .map_err(other)
}

pub async fn tts_key_present(b: &LiveBackend, provider: TtsProviderVm) -> UiResult<bool> {
    if !provider.needs_key() {
        return Ok(false);
    }
    Ok(secret(b, provider).await?.is_some())
}

fn select_kitten(b: &LiveBackend) {
    let prefs = settings_store::load(&b.settings_file).novel.tts;
    b.tts.select_kitten_model(&kitten_model(&prefs));
}

pub async fn kitten_status(b: &LiveBackend) -> UiResult<KittenStatusVm> {
    select_kitten(b);
    b.tts.kitten_status().await
}

pub async fn kitten_install(b: &LiveBackend) -> UiResult<KittenStatusVm> {
    select_kitten(b);
    b.tts.kitten_install().await
}

pub async fn kitten_remove(b: &LiveBackend) -> UiResult<()> {
    b.tts.kitten_remove().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::{Media, MediaKind, MediaRef};

    fn info_of(titles: &[&str]) -> NovelInfo {
        NovelInfo {
            media: Media {
                media_ref: MediaRef {
                    anilist_id: Some(85737),
                    mal_id: Some(21479),
                    kitsu_id: None,
                },
                kind: MediaKind::Manga,
                title: "Overlord".into(),
                titles: Default::default(),
                cover: None,
                banner: None,
                color: None,
                synopsis: None,
                genres: vec![],
                episodes: None,
                status: None,
                season: None,
                year: Some(2010),
                score: None,
                source_provider: "anilist".into(),
            },
            format: Some("NOVEL".into()),
            chapters: None,
            volumes: Some(16),
            authors: vec![],
            country: None,
            adult: false,
            titles: titles.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn ids_params_and_cards() {
        assert_eq!(anilist_id("85737").unwrap(), 85737);
        assert_eq!(anilist_id("anilist:5").unwrap(), 5);
        assert!(anilist_id("mal:5").is_err());
        let i = info_of(&["Overlord", "オーバーロード"]);
        let p = chapter_params(&i, true);
        assert_eq!(p["anilistId"], 85737);
        assert_eq!(p["title"], "Overlord");
        assert_eq!(p["fullScan"], true);
        assert_eq!(chapter_params(&info_of(&[]), false)["title"], "Overlord");
        let vm = novel_vm(&i);
        assert_eq!(vm.id, "85737");
        assert_eq!(vm.subtitle, "2010 - 16 vol");
    }

    #[test]
    fn search_uses_the_origin_tag() {
        let q = SearchQueryVm {
            query: "x".into(),
            tags: vec!["origin:kr".into()],
            page: 0,
            ..SearchQueryVm::default()
        };
        let s = search_of(&q);
        assert_eq!(s.origin.as_deref(), Some("KR"));
        assert_eq!(s.page, 1);
        assert_eq!(s.sort, "relevance");
    }

    #[test]
    fn offline_text_round_trip_and_labels() {
        let j = offline_json("Start", &["a".into(), "b".into()]);
        assert_eq!(
            parse_offline(&j),
            Some(("Start".into(), vec!["a".into(), "b".into()]))
        );
        assert_eq!(parse_offline("{\"paragraphs\":[]}"), None);
        assert_eq!(chapter_label(3.0, " The end "), "Ch. 3 · The end");
        assert_eq!(chapter_label(3.5, ""), "Ch. 3.5");
    }

    #[test]
    fn providers_group_and_filter() {
        let row = |p: &str, n: f32, tl: Option<&str>| ProviderChapter {
            provider: p.into(),
            provider_name: None,
            chapter_key: format!("{p}:{n}"),
            provider_chapter_id: String::new(),
            number: n,
            title: None,
            volume: None,
            language: Some("en".into()),
            translator: tl.map(str::to_owned),
            published_unix: None,
        };
        let ext = NovelExt {
            id: "toko".into(),
            name: "Toko".into(),
        };
        let rows = vec![
            row("a", 1.0, Some("TL")),
            row("a", 2.0, None),
            row("b", 1.0, Some("tl")),
        ];
        let first_id = chapter_key("1", &rows[0]).encode();
        let offline: HashSet<(String, String)> = [("toko".to_owned(), first_id.clone())].into();
        let progress = vec![NovelProgressRow {
            media_id: "1".into(),
            ext_id: "toko".into(),
            chapter_id: first_id,
            chapter_number: 1.0,
            position: 5,
            total: Some(10),
            completed: false,
            title: None,
            poster: None,
            updated_at: 1,
        }];
        let all = provider_vms("1", &ext, rows.clone(), None, false, &offline, &progress);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].ext_name, "a");
        assert!(all[0].chapters[0].downloaded);
        assert_eq!(all[0].chapters[0].read, 0.5);
        let tl = provider_vms("1", &ext, rows, Some("tl"), true, &offline, &progress);
        assert_eq!(
            tl.iter()
                .map(|p| (p.ext_name.as_str(), p.chapters.len()))
                .collect::<Vec<_>>(),
            vec![("a · Toko", 1), ("b · Toko", 1)]
        );
    }
}
