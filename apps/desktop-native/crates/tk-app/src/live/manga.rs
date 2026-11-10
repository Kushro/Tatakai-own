//! Manga + the reading calls shared with novels (wave B, worker **l**; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `manga.*`). tk-core side: `tk_core::manga` (chapter lists per provider), the metadata
//! clients `tk_core::metadata::{mangadex, mangaupdates}` and `tk_core::metadata::anilist::manga`, and the
//! `LocalStore` tables of `tk-core/src/store/reading.rs`. `read_*` take `ReadKindVm` so worker m reuses them for
//! novels without copying.
//!
//! - **Ids**: a manga is its AniList id (`"30013"`, same id space as anime); titles only MangaDex knows are
//!   `md:<uuid>` (`tk_core::metadata::mangadex::ID_PREFIX`).
//! - **Search / home** (`manga.search`, `manga.home`, `manga.metadata.anilist`): AniList `type: MANGA` pages; when
//!   AniList fails, MangaDex answers text searches and the trending row (`manga.metadata.mangadex`). Conventions of
//!   `SearchQueryVm` for manga: `season` = origin (`MANGA_ORIGINS`), `format` = `MANGA | ONE_SHOT`, `tags` may
//!   carry `ADULT_ONLY_TAG` (18+ zone). 18+ cards get the `ADULT_BADGE` subtitle prefix.
//! - **Details** (`manga.details`, `manga.metadata.mangaupdates`): AniList details + MangaUpdates (publisher,
//!   serialisation, publication status, Bayesian rating) + the MangaDex id, fetched concurrently with a time budget
//!   (a slow MangaUpdates never blocks the page).
//! - **Chapters** (`manga.chapters.providers`): every enabled extension whose manifest declares manga (`capabilities`
//!   `manga` / `chapters`, route `manga/chapters` or type `manga`) is asked through `ExtensionHost::call`, plus the
//!   built-in MangaDex provider (`@mangadex`, preferred language + English). One `ChapterProviderVm` per inner
//!   provider (`<ext_id>/<provider>`), cached 6 h in `chapter_lists`; failed providers carry their error and keep
//!   their stale cache when there is one. Providers are ordered by reliability (successful page loads / chapter
//!   lists this session), so the UI's `merge_chapters` breaks ties with it (`manga.chapters.auto-best`).
//! - **Pages**: the offline copy first (`offline_chapters`), else the extension's `manga/pages` or MangaDex
//!   at-home. The web view loads them through the reader's `tkpage` asset handler with the page headers
//!   ([`LivePageIo`], installed into `tk_ui::components::reader` on first use).
//! - **Progress / list** (`manga.reader.progress`, `manga.readlist`, `manga.readlist.anilist-sync`): positions in
//!   `read_progress`; the read list is the AniList manga list (`MediaListCollection(type: MANGA)`) mirrored in
//!   `read_list_entries` with the offline queue `read_list_queue`. Without an AniList account the list is local.
//!   Finishing a chapter raises the entry's progress when `Settings::lists.auto_progress`.
//! - **Downloads** (`manga.download.chapters`): chapters are fetched page by page in a background task into
//!   `<Settings::downloads.dir or Downloads/Tatakai>/Manga/<title>/<chapter>/`, registered in `offline_chapters` and
//!   listed in the shared `downloads` table (kind `manga`), so worker h's Downloads page shows them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use time::OffsetDateTime;
use tk_core::lists::{Service, Token, anilist_status_to_shared, shared_status_to_anilist};
use tk_core::manga::{ChapterList, Page, parse_ext_chapters, parse_ext_pages};
use tk_core::metadata::anilist::AniList;
use tk_core::metadata::anilist::lists::EntryInput;
use tk_core::metadata::anilist::manga::{MangaCard, MangaSearch, map_manga_card, map_manga_details};
use tk_core::metadata::mangadex::{self, MangaDex, MdManga};
use tk_core::metadata::mangaupdates::{MangaUpdates, MuSeries};
use tk_core::store::reading::{ChapterDownloadRow, OfflineChapterRow, ReadListRow, ReadProgressRow};
use tk_core::{Compat, ListError, LocalStore, Media, ProviderError};
use tk_ext::ExtensionHost;
use tk_ui::backend::{
    ChapterProviderVm, ChapterRefVm, ChapterVm, CharacterRoleVm, ContinueReadVm, ListEntryUpdateVm,
    ListEntryVm, ListStatusVm, MangaDetailsVm, MediaVm, PageVm, ReadHomeVm, ReadProgressVm, RelatedVm,
    SearchPageVm, SearchQueryVm, SectionVm, UiResult,
};
use tk_ui::components::reader::{IoFuture, PageIo, install_page_io};
use tk_ui::model::manga::{
    ADULT_BADGE, ADULT_ONLY_TAG, MANGADEX_PROVIDER, OFFLINE_URL_PREFIX, chapter_label,
};
use tk_ui::model::{ErrorKind, ListServiceVm, ReadKindVm, Settings, UiError};

use crate::live::LiveBackend;
use crate::settings_store;

/// Cached chapter lists are reused for this long (Refresh bypasses it).
const CHAPTER_TTL_SECS: i64 = 6 * 3600;
/// Time budget of one extension's `manga/chapters` / `manga/pages` call.
const EXT_TIMEOUT: Duration = Duration::from_secs(25);
/// Budget of the optional details enrichments (MangaUpdates, MangaDex id).
const ENRICH_TIMEOUT: Duration = Duration::from_secs(8);
/// Budget of an interactive AniList list write (the op stays queued when it runs out).
const PUSH_BUDGET: Duration = Duration::from_secs(4);
/// Titles the followed-updates row checks.
const FOLLOWED_LIMIT: usize = 8;
/// A queued list op that keeps failing for a non-network reason is dropped after this many attempts.
const MAX_ATTEMPTS: u32 = 5;
/// Cards per home row / search page.
const PAGE_SIZE: u32 = 24;

const OP_UPSERT: &str = "upsert";
const OP_DELETE: &str = "delete";

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

fn anilist() -> AniList {
    AniList::new(client().clone())
}

fn mangadex_client() -> MangaDex {
    MangaDex::new(client().clone())
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn other(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn prov_err(e: ProviderError) -> UiError {
    super::provider_err(e)
}

fn settings(b: &LiveBackend) -> Settings {
    settings_store::load(&b.settings_file)
}

/// Title language of manga cards: `MangaPrefs::title_language`, else the app-wide one (`manga.title-language`).
fn title_language(s: &Settings) -> String {
    if s.manga.title_language.is_empty() {
        s.title_language.clone()
    } else {
        s.manga.title_language.clone()
    }
}

fn display(m: &Media, lang: &str) -> String {
    if lang.is_empty() {
        m.title.clone()
    } else {
        super::details::display_title(m, lang)
    }
}

/// A manga id of the UI.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MangaId {
    AniList(u32),
    MangaDex(String),
}

fn parse_id(id: &str) -> Option<MangaId> {
    let id = id.trim();
    if let Some(md) = id.strip_prefix(mangadex::ID_PREFIX) {
        return (!md.is_empty()).then(|| MangaId::MangaDex(md.to_owned()));
    }
    id.parse::<u32>().ok().filter(|n| *n > 0).map(MangaId::AniList)
}

fn invalid(id: &str) -> UiError {
    UiError::new(ErrorKind::Other, format!("invalid manga id {id}"))
}

/// What the chapter, progress and list calls need to know about a title (remembered by search / details / home).
#[derive(Debug, Clone, PartialEq, Default)]
struct TitleInfo {
    anilist_id: Option<u32>,
    mal_id: Option<u32>,
    md_id: Option<String>,
    title: String,
    titles: Vec<String>,
    poster: Option<String>,
    chapters: Option<u32>,
    year: Option<i32>,
}

fn titles() -> &'static Mutex<HashMap<String, TitleInfo>> {
    static T: OnceLock<Mutex<HashMap<String, TitleInfo>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(HashMap::new()))
}

fn remember(id: &str, info: TitleInfo) {
    if let Ok(mut t) = titles().lock() {
        let entry = t.entry(id.to_owned()).or_default();
        // Keep what an earlier, richer answer knew (a card has no MangaDex id, details do).
        let md = info.md_id.clone().or_else(|| entry.md_id.clone());
        *entry = TitleInfo { md_id: md, ..info };
    }
}

fn recall(id: &str) -> Option<TitleInfo> {
    titles().lock().ok().and_then(|t| t.get(id).cloned())
}

fn info_of(media: &Media, lang: &str, chapters: Option<u32>) -> TitleInfo {
    let mut all: Vec<String> = vec![media.title.clone()];
    for t in media.titles.values() {
        if !all.contains(t) {
            all.push(t.clone());
        }
    }
    TitleInfo {
        anilist_id: media.media_ref.anilist_id,
        mal_id: media.media_ref.mal_id,
        md_id: None,
        title: display(media, lang),
        titles: all,
        poster: media.cover.clone(),
        chapters,
        year: media.year,
    }
}

/// `2019 - 120 ch` style subtitle; 18+ titles start with [`ADULT_BADGE`].
fn subtitle(adult: bool, year: Option<i32>, chapters: Option<u32>, volumes: Option<u32>) -> String {
    let mut parts = Vec::new();
    if adult {
        parts.push(ADULT_BADGE.to_owned());
    }
    if let Some(y) = year {
        parts.push(y.to_string());
    }
    if let Some(c) = chapters {
        parts.push(format!("{c} ch"));
    } else if let Some(v) = volumes {
        parts.push(format!("{v} vol"));
    }
    parts.join(" - ")
}

fn card_vm(c: &MangaCard, lang: &str) -> MediaVm {
    let id = c
        .media
        .media_ref
        .anilist_id
        .map(|a| a.to_string())
        .unwrap_or_default();
    remember(&id, info_of(&c.media, lang, c.chapters));
    MediaVm {
        id,
        title: display(&c.media, lang),
        cover: c.media.cover.clone(),
        color: c.media.color.clone(),
        subtitle: subtitle(c.adult, c.media.year, c.chapters, c.volumes),
        score: c.media.score,
    }
}

fn md_vm(m: &MdManga, lang: &str) -> MediaVm {
    let id = match m.media.media_ref.anilist_id {
        Some(a) => a.to_string(),
        None => format!("{}{}", mangadex::ID_PREFIX, m.id),
    };
    let mut info = info_of(&m.media, lang, None);
    info.md_id = Some(m.id.clone());
    info.titles.extend(m.alt_titles.iter().take(6).cloned());
    remember(&id, info);
    MediaVm {
        id,
        title: display(&m.media, lang),
        cover: m.media.cover.clone(),
        color: None,
        subtitle: subtitle(m.adult, m.media.year, m.last_chapter.map(|n| n as u32), None),
        score: None,
    }
}

// ---------------------------------------------------------------------------------------------
// Search and home
// ---------------------------------------------------------------------------------------------

/// `SearchQueryVm` (manga conventions, module docs) -> AniList variables input.
fn manga_query(q: &SearchQueryVm, per_page: u32) -> MangaSearch {
    let adult_only = q.tags.iter().any(|t| t == ADULT_ONLY_TAG);
    MangaSearch {
        query: q.query.clone(),
        genres: q.genres.clone(),
        tags: q
            .tags
            .iter()
            .filter(|t| t.as_str() != ADULT_ONLY_TAG)
            .cloned()
            .collect(),
        country: q.season.clone(),
        status: q.status.clone(),
        format: q.format.clone(),
        novel: false,
        year: q.year,
        sort: q.sort.id().to_owned(),
        adult: if adult_only {
            Some(true)
        } else if q.adult {
            Some(false)
        } else {
            None
        },
        page: q.page.max(1),
        per_page,
    }
}

pub async fn search(b: &LiveBackend, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    let s = settings(b);
    let lang = title_language(&s);
    match anilist().manga_search(&manga_query(q, PAGE_SIZE)).await {
        Ok(p) => Ok(SearchPageVm {
            items: p.items.iter().map(|c| card_vm(c, &lang)).collect(),
            page: p.page,
            has_next: p.has_next,
            total: p.total,
        }),
        // AniList down: MangaDex answers plain text searches (`manga.metadata.mangadex`).
        Err(e) if !q.query.trim().is_empty() => {
            tracing::warn!(error = %e, "AniList manga search failed; falling back to MangaDex");
            let adult = q.adult || q.tags.iter().any(|t| t == ADULT_ONLY_TAG);
            let (items, has_next) = mangadex_client()
                .search_titles(&q.query, q.page, adult)
                .await
                .map_err(prov_err)?;
            Ok(SearchPageVm {
                items: items.iter().map(|m| md_vm(m, &lang)).collect(),
                page: q.page.max(1),
                has_next,
                total: None,
            })
        }
        Err(e) => Err(prov_err(e)),
    }
}

/// One home row: `(feed id, search)`; the row key is `manga.feed.<id>`.
fn home_rows() -> Vec<(&'static str, SearchQueryVm)> {
    use tk_ui::backend::SearchSortVm;
    let q = |sort: SearchSortVm| SearchQueryVm {
        sort,
        page: 1,
        ..SearchQueryVm::default()
    };
    vec![
        ("trending", q(SearchSortVm::Trending)),
        ("popular", q(SearchSortVm::Popularity)),
        (
            "releasing",
            SearchQueryVm {
                status: Some("releasing".into()),
                ..q(SearchSortVm::Trending)
            },
        ),
        (
            "manhwa",
            SearchQueryVm {
                season: Some("kr".into()),
                ..q(SearchSortVm::Trending)
            },
        ),
        (
            "manhua",
            SearchQueryVm {
                season: Some("cn".into()),
                ..q(SearchSortVm::Trending)
            },
        ),
        ("top_rated", q(SearchSortVm::Score)),
        (
            "one_shot",
            SearchQueryVm {
                format: Some("ONE_SHOT".into()),
                ..q(SearchSortVm::Popularity)
            },
        ),
    ]
}

async fn home_row(b: &LiveBackend, id: &str, q: SearchQueryVm) -> SectionVm {
    let items = match search(b, &q).await {
        Ok(p) => Ok(p.items),
        Err(e) if id == "trending" => {
            // AniList down: MangaDex's most-followed titles keep the hero alive.
            let lang = title_language(&settings(b));
            match mangadex_client().popular(1).await {
                Ok((list, _)) => Ok(list.iter().map(|m| md_vm(m, &lang)).collect()),
                Err(_) => Err(e),
            }
        }
        Err(e) => Err(e),
    };
    SectionVm {
        title_key: format!("manga.feed.{id}"),
        items,
    }
}

fn media_of_row(id: &str, title: Option<&str>, poster: Option<&str>) -> MediaVm {
    let info = recall(id);
    MediaVm {
        id: id.to_owned(),
        title: title
            .map(str::to_owned)
            .or_else(|| info.as_ref().map(|i| i.title.clone()))
            .unwrap_or_else(|| id.to_owned()),
        cover: poster.map(str::to_owned).or_else(|| info.and_then(|i| i.poster)),
        color: None,
        subtitle: String::new(),
        score: None,
    }
}

fn progress_vm(r: &ReadProgressRow) -> ReadProgressVm {
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

pub async fn home(b: &LiveBackend) -> UiResult<ReadHomeVm> {
    ensure_page_io(b);
    let continue_reading = b
        .store
        .continue_reading(ReadKindVm::Manga.id(), 20)
        .map_err(other)?
        .iter()
        .map(|r| ContinueReadVm {
            media: media_of_row(&r.media_id, r.title.as_deref(), r.poster.as_deref()),
            kind: ReadKindVm::Manga,
            progress: progress_vm(r),
        })
        .collect();
    ensure_pulled(b).await;
    let continue_reading = with_list_items(b, ReadKindVm::Manga, continue_reading, 20);
    let rows = home_rows();
    let mut sections = Vec::with_capacity(rows.len() + 1);
    let followed = followed_updates(b);
    let (updates, mut feeds) = tokio::join!(followed, async {
        let mut out = Vec::new();
        for (id, q) in rows {
            out.push(home_row(b, id, q).await);
        }
        out
    });
    // The trending row stays first (the hero takes it); followed updates come right after it.
    if !feeds.is_empty() {
        sections.push(feeds.remove(0));
    }
    if matches!(&updates, Ok(items) if !items.is_empty()) {
        sections.push(SectionVm {
            title_key: "manga.feed.updates".into(),
            items: updates,
        });
    }
    sections.extend(feeds);
    Ok(ReadHomeVm {
        continue_reading,
        sections,
    })
}

/// New chapters of the titles being read (`manga.home.followed-updates`): read-list entries in Reading /
/// Rereading / Paused plus the titles of "Continue reading", newest first; a title shows when its providers list
/// chapters above the last one read. Cards carry `+<new> - <latest>` as subtitle.
async fn followed_updates(b: &LiveBackend) -> Result<Vec<MediaVm>, UiError> {
    let kind = ReadKindVm::Manga.id();
    let mut cands: Vec<(String, Option<String>, Option<String>, f32)> = Vec::new();
    for e in b.store.read_list_entries(kind, None).map_err(other)? {
        if matches!(e.status.as_str(), "watching" | "repeating" | "paused") {
            cands.push((
                e.media_id.clone(),
                Some(e.title.clone()),
                e.poster.clone(),
                e.progress as f32,
            ));
        }
    }
    for r in b.store.continue_reading(kind, 20).map_err(other)? {
        if !cands.iter().any(|c| c.0 == r.media_id) {
            cands.push((
                r.media_id.clone(),
                r.title.clone(),
                r.poster.clone(),
                r.chapter_number,
            ));
        }
    }
    cands.truncate(FOLLOWED_LIMIT);
    let ctx = ChapterCtx::new(b);
    let mut set = tokio::task::JoinSet::new();
    for (media_id, title, poster, listed) in cands {
        let ctx = ctx.clone();
        let read_max = b
            .store
            .read_progress(kind, &media_id)
            .map(|rows| {
                rows.iter()
                    .filter(|r| r.completed)
                    .map(|r| r.chapter_number)
                    .fold(0.0_f32, f32::max)
            })
            .unwrap_or(0.0)
            .max(listed);
        set.spawn(async move {
            let key = match ctx.key_for(&media_id).await {
                Ok(k) => k,
                Err(_) => return None,
            };
            let lists = ctx.lists(&media_id, &key, false).await;
            let all: Vec<ChapterList> = lists.into_iter().filter_map(|(_, r)| r.ok()).flatten().collect();
            let latest = tk_core::manga::latest_number(&all);
            let mut numbers: Vec<f32> = all
                .iter()
                .flat_map(|l| l.chapters.iter().map(|c| c.number))
                .filter(|n| *n > read_max + 0.004)
                .collect();
            numbers.sort_by(f32::total_cmp);
            numbers.dedup_by(|a, b| tk_core::manga::same_number(*a, *b));
            (read_max > 0.0 && !numbers.is_empty()).then(|| {
                let mut m = media_of_row(&media_id, title.as_deref(), poster.as_deref());
                m.subtitle = format!("+{} - {}", numbers.len(), chapter_label(latest));
                m
            })
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(Some(m)) = r {
            out.push(m);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Details
// ---------------------------------------------------------------------------------------------

fn fuzzy((y, m, d): (Option<i32>, Option<u32>, Option<u32>)) -> Option<String> {
    match (y, m, d) {
        (Some(y), Some(m), Some(d)) => Some(format!("{y:04}-{m:02}-{d:02}")),
        (Some(y), Some(m), None) => Some(format!("{y:04}-{m:02}")),
        (Some(y), None, _) => Some(y.to_string()),
        _ => None,
    }
}

fn status_id(raw: Option<&str>) -> Option<String> {
    raw.map(|s| match s {
        "NOT_YET_RELEASED" => "upcoming".to_owned(),
        other => other.to_ascii_lowercase(),
    })
}

/// Facts from MangaUpdates (`manga.metadata.mangaupdates`).
fn mu_facts(
    mu: &MuSeries,
    facts: &mut Vec<(String, String)>,
    ratings: &mut Vec<(String, u32)>,
    external: &mut Vec<(String, String)>,
) {
    if !mu.publishers.is_empty() {
        facts.push(("publisher".into(), mu.publishers.join(", ")));
    }
    if !mu.serialization.is_empty() {
        facts.push(("serialization".into(), mu.serialization.join(", ")));
    }
    if let Some(st) = &mu.status {
        facts.push(("publication".into(), st.clone()));
    }
    if !mu.licensed.is_empty() {
        facts.push(("licensed".into(), mu.licensed.join(", ")));
    }
    if let Some(r) = mu.rating {
        ratings.push(("MangaUpdates".into(), r));
    }
    if let Some(u) = &mu.url {
        external.push(("MangaUpdates".into(), u.clone()));
    }
}

/// AniList-first "continue reading" (2026-11-10): the reader positions plus the titles being read on the list
/// (Reading / Rereading, started, not finished) that have no position here. A list-only item has no chapter reference (empty
/// `ext_id`): the card opens the title instead of the reader and shows "Next: chapter P + 1". Newest first.
pub(crate) fn with_list_items(b: &LiveBackend, kind: ReadKindVm, items: Vec<ContinueReadVm>, limit: usize) -> Vec<ContinueReadVm> {
    let have: HashSet<String> = items.iter().map(|c| c.media.id.clone()).collect();
    let mut all = items;
    for r in b.store.read_list_entries(kind.id(), None).unwrap_or_default() {
        if !matches!(r.status.as_str(), "watching" | "repeating")
            || r.progress == 0
            || r.chapters.is_some_and(|c| c > 0 && r.progress >= c)
            || have.contains(&r.media_id)
        {
            continue;
        }
        all.push(ContinueReadVm {
            media: media_of_row(&r.media_id, Some(&r.title), r.poster.as_deref()),
            kind,
            progress: ReadProgressVm {
                chapter: ChapterRefVm { ext_id: String::new(), chapter_id: String::new() },
                chapter_number: (r.progress + 1) as f32,
                position: 0,
                total: None,
                completed: false,
                updated_unix: r.updated_at,
            },
        });
    }
    all.sort_by(|a, b| b.progress.updated_unix.cmp(&a.progress.updated_unix));
    all.truncate(limit);
    all
}

fn resume_of(b: &LiveBackend, kind: ReadKindVm, id: &str) -> Option<ReadProgressVm> {
    b.store
        .read_progress(kind.id(), id)
        .ok()
        .and_then(|rows| rows.first().map(progress_vm))
}

pub async fn details(b: &LiveBackend, id: &str) -> UiResult<MangaDetailsVm> {
    ensure_page_io(b);
    // Wave B (j, `ext.custom-providers.read`): provider titles of kind `read` come from their extension.
    if super::ext_catalog::is_provider_id(id) {
        return super::ext_catalog::provider_manga_details(b, id).await;
    }
    let s = settings(b);
    let lang = title_language(&s);
    match parse_id(id).ok_or_else(|| invalid(id))? {
        MangaId::AniList(al) => details_anilist(b, id, al, &lang).await,
        MangaId::MangaDex(md) => details_mangadex(b, id, &md, &lang).await,
    }
}

async fn details_anilist(b: &LiveBackend, id: &str, al: u32, lang: &str) -> UiResult<MangaDetailsVm> {
    let raw = anilist()
        .manga_details_raw(al)
        .await
        .map_err(prov_err)?
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("{id} was not found on AniList")))?;
    let d = map_manga_details(&raw).ok_or_else(|| invalid(id))?;
    let mut info = info_of(&d.card.media, lang, d.card.chapters);
    let year = d.card.media.year;
    let titles_for_lookup = info.titles.clone();
    let (mu_client, md_client) = (MangaUpdates::new(client().clone()), mangadex_client());
    let (mu, md) = tokio::join!(
        tokio::time::timeout(ENRICH_TIMEOUT, mu_client.lookup(&titles_for_lookup, year)),
        tokio::time::timeout(ENRICH_TIMEOUT, md_client.find(Some(al), info.mal_id, &titles_for_lookup)),
    );
    let mu = match mu {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "MangaUpdates lookup failed");
            None
        }
        Err(_) => None,
    };
    let md_id = match md {
        Ok(Ok(m)) => m,
        _ => None,
    };
    info.md_id = md_id.clone();
    remember(id, info.clone());

    let x = &d.extra;
    let mut facts: Vec<(String, String)> = Vec::new();
    if let Some(v) = fuzzy(x.start_date) {
        facts.push(("start".into(), v));
    }
    if let Some(v) = fuzzy(x.end_date) {
        facts.push(("end".into(), v));
    }
    if let Some(c) = &x.country {
        facts.push(("origin".into(), c.clone()));
    }
    if let Some(src) = &x.source {
        facts.push(("source".into(), src.clone()));
    }
    if let Some(p) = x.popularity {
        facts.push(("popularity".into(), p.to_string()));
    }
    if let Some(f) = x.favourites {
        facts.push(("favourites".into(), f.to_string()));
    }
    let mut ratings: Vec<(String, u32)> = Vec::new();
    if let Some(sc) = x.mean_score.or(d.card.media.score) {
        ratings.push(("AniList".into(), sc));
    }
    let mut external: Vec<(String, String)> = Vec::new();
    external.push((
        "AniList".into(),
        d.site_url
            .clone()
            .unwrap_or_else(|| format!("https://anilist.co/manga/{al}")),
    ));
    if let Some(m) = &mu {
        mu_facts(m, &mut facts, &mut ratings, &mut external);
    }
    if let Some(m) = &md_id {
        external.push(("MangaDex".into(), format!("https://mangadex.org/title/{m}")));
    }
    for l in &x.external_links {
        if !external.iter().any(|(_, u)| *u == l.1) {
            external.push(l.clone());
        }
    }
    let mut source_ids = vec![("anilist".to_owned(), al.to_string())];
    if let Some(m) = info.mal_id {
        source_ids.push(("mal".into(), m.to_string()));
    }
    if let Some(m) = &md_id {
        source_ids.push(("mangadex".into(), m.clone()));
    }
    if let Some(m) = &mu {
        source_ids.push(("mangaupdates".into(), m.id.to_string()));
    }
    let relations = d
        .relations
        .iter()
        .map(|(rel, card, is_anime)| RelatedVm {
            media: card_vm(card, lang),
            relation: rel.to_ascii_lowercase(),
            kind: if *is_anime { "anime".into() } else { "manga".into() },
        })
        .collect();
    let characters = x
        .characters
        .iter()
        .map(|c| CharacterRoleVm {
            id: c.id.to_string(),
            name: c.name.clone(),
            image: c.image.clone(),
            role: c.role.clone(),
            voice_actors: Vec::new(),
        })
        .collect();
    let media = card_vm(&d.card, lang);
    Ok(MangaDetailsVm {
        banner: d.card.media.banner.clone(),
        synopsis: d.card.media.synopsis.clone(),
        genres: d.card.media.genres.clone(),
        status: status_id(d.card.media.status.as_deref()),
        format: d.card.format.clone(),
        chapters_total: d.card.chapters.or(mu.as_ref().and_then(|m| m.latest_chapter)),
        volumes_total: d.card.volumes,
        authors: d.authors.clone(),
        anime_id: d.anime_id.map(|a| a.to_string()),
        external,
        ratings,
        adult: d.card.adult,
        resume: resume_of(b, ReadKindVm::Manga, id),
        tags: x.tags.clone(),
        characters,
        recommendations: d.recommendations.iter().map(|c| card_vm(c, lang)).collect(),
        relations,
        facts,
        synonyms: x.synonyms.clone(),
        source_ids,
        media,
    })
}

async fn details_mangadex(b: &LiveBackend, id: &str, md: &str, lang: &str) -> UiResult<MangaDetailsVm> {
    let m = mangadex_client()
        .manga(md)
        .await
        .map_err(prov_err)?
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("{id} was not found on MangaDex")))?;
    let mut media = md_vm(&m, lang);
    media.id = id.to_owned();
    let mut info = info_of(&m.media, lang, None);
    info.md_id = Some(m.id.clone());
    info.titles.extend(m.alt_titles.iter().take(6).cloned());
    remember(id, info);
    let mut facts = Vec::new();
    if let Some(o) = &m.original_language {
        facts.push(("origin".to_owned(), o.to_ascii_uppercase()));
    }
    if let Some(y) = m.media.year {
        facts.push(("start".to_owned(), y.to_string()));
    }
    Ok(MangaDetailsVm {
        media,
        banner: None,
        synopsis: m.media.synopsis.clone(),
        genres: m.media.genres.clone(),
        status: status_id(m.media.status.as_deref()),
        format: None,
        chapters_total: m.last_chapter.map(|n| n as u32),
        volumes_total: None,
        authors: m.authors.clone(),
        anime_id: None,
        external: vec![("MangaDex".into(), format!("https://mangadex.org/title/{}", m.id))],
        ratings: Vec::new(),
        adult: m.adult,
        resume: resume_of(b, ReadKindVm::Manga, id),
        tags: m.tags.iter().map(|t| (t.clone(), 0)).collect(),
        characters: Vec::new(),
        recommendations: Vec::new(),
        relations: Vec::new(),
        facts,
        synonyms: m.alt_titles.iter().take(8).cloned().collect(),
        source_ids: vec![("mangadex".into(), m.id.clone())],
    })
}

// ---------------------------------------------------------------------------------------------
// Chapters
// ---------------------------------------------------------------------------------------------

/// Session reliability per extension id: `(ok, failed)` page loads / chapter lists.
fn reliability_map() -> &'static Mutex<HashMap<String, (u32, u32)>> {
    static R: OnceLock<Mutex<HashMap<String, (u32, u32)>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn record(ext_id: &str, ok: bool) {
    if let Ok(mut m) = reliability_map().lock() {
        let e = m.entry(ext_id.to_owned()).or_insert((0, 0));
        if ok {
            e.0 += 1;
        } else {
            e.1 += 1;
        }
    }
}

/// Laplace-smoothed success ratio (0.5 for an unknown provider).
fn reliability(ext_id: &str) -> f32 {
    let (ok, failed) = reliability_map()
        .lock()
        .ok()
        .and_then(|m| m.get(ext_id).copied())
        .unwrap_or((0, 0));
    (ok as f32 + 1.0) / ((ok + failed) as f32 + 2.0)
}

/// Whether a manifest declares manga support.
fn manifest_reads_manga(manifest: &Value) -> bool {
    let has = |key: &str, what: &[&str]| {
        manifest[key]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str().is_some_and(|s| what.contains(&s))))
    };
    manifest["type"] == "manga"
        || has("capabilities", &["manga", "chapters"])
        || manifest["apiServer"]["routes"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "manga/chapters"))
}

/// Enabled, runnable extensions that read manga: `(id, name)`.
fn manga_extensions(b: &LiveBackend, host: &ExtensionHost) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = b
        .ext
        .installed()
        .into_iter()
        .filter(|e| e.enabled && e.compat != Compat::Unsupported)
        .filter(|e| {
            std::fs::read_to_string(host.root().join(&e.id).join("manifest.json"))
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
                .is_some_and(|m| manifest_reads_manga(&m))
        })
        .map(|e| (e.id, e.name))
        .collect();
    out.sort();
    out
}

/// Owned context of the chapter fetches (runs in spawned tasks).
#[derive(Clone)]
struct ChapterCtx {
    store: Arc<LocalStore>,
    host: Arc<ExtensionHost>,
    exts: Vec<(String, String)>,
    lang: String,
    adult: bool,
}

/// What a provider needs to find a title.
#[derive(Debug, Clone, Default)]
struct TitleKey {
    anilist_id: Option<u32>,
    mal_id: Option<u32>,
    md_id: Option<String>,
    titles: Vec<String>,
}

impl ChapterCtx {
    fn new(b: &LiveBackend) -> Self {
        let host = b.ext.host();
        let s = settings(b);
        Self {
            store: b.store.clone(),
            exts: manga_extensions(b, &host),
            host,
            lang: s.manga.preferred_lang.clone(),
            adult: s.manga.adult_ok || s.home.mature_content == tk_ui::model::home::MatureContentVm::Show,
        }
    }

    /// The title's key from memory, or from AniList / MangaDex when this session has not seen it.
    async fn key_for(&self, media_id: &str) -> UiResult<TitleKey> {
        if let Some(i) = recall(media_id) {
            return Ok(TitleKey {
                anilist_id: i.anilist_id,
                mal_id: i.mal_id,
                md_id: i.md_id,
                titles: i.titles,
            });
        }
        match parse_id(media_id).ok_or_else(|| invalid(media_id))? {
            MangaId::MangaDex(md) => Ok(TitleKey {
                md_id: Some(md),
                ..TitleKey::default()
            }),
            MangaId::AniList(al) => {
                let raw = anilist()
                    .manga_details_raw(al)
                    .await
                    .map_err(prov_err)?
                    .ok_or_else(|| invalid(media_id))?;
                let card = map_manga_card(&raw);
                let info = info_of(&card.media, "", card.chapters);
                remember(media_id, info.clone());
                Ok(TitleKey {
                    anilist_id: Some(al),
                    mal_id: info.mal_id,
                    md_id: None,
                    titles: info.titles,
                })
            }
        }
    }

    fn cached(&self, media_id: &str, ext_id: &str, max_age: i64) -> Option<Vec<ChapterList>> {
        let kind = ReadKindVm::Manga.id();
        let row = if max_age == i64::MAX {
            self.store
                .chapter_lists(kind, media_id)
                .ok()?
                .into_iter()
                .find(|(e, _, _)| e == ext_id)
                .map(|(_, j, _)| j)
        } else {
            self.store
                .chapter_list(kind, media_id, ext_id, max_age, now())
                .ok()?
                .map(|(j, _)| j)
        };
        row.and_then(|j| serde_json::from_str(&j).ok())
    }

    fn store_lists(&self, media_id: &str, ext_id: &str, lists: &[ChapterList]) {
        if let Ok(j) = serde_json::to_string(lists)
            && let Err(e) = self
                .store
                .set_chapter_list(ReadKindVm::Manga.id(), media_id, ext_id, &j, now())
        {
            tracing::warn!(error = %e, "could not cache a chapter list");
        }
    }

    async fn fetch_ext(&self, ext_id: &str, key: &TitleKey) -> Result<Vec<ChapterList>, String> {
        let params = json!({
            "anilistId": key.anilist_id,
            "malId": key.mal_id,
            "title": key.titles.first(),
            "titles": key.titles,
        });
        match tokio::time::timeout(EXT_TIMEOUT, self.host.call(ext_id, "manga/chapters", params)).await {
            Ok(Ok(body)) => Ok(parse_ext_chapters(ext_id, &body)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("timeout".into()),
        }
    }

    async fn fetch_mangadex(&self, key: &TitleKey) -> Result<Vec<ChapterList>, String> {
        let md = MangaDex::new(client().clone());
        let id = match &key.md_id {
            Some(id) => id.clone(),
            None => match md.find(key.anilist_id, key.mal_id, &key.titles).await {
                Ok(Some(id)) => id,
                Ok(None) => return Ok(Vec::new()),
                Err(e) => return Err(e.to_string()),
            },
        };
        let mut langs = vec![self.lang.clone()];
        if self.lang != "en" {
            langs.push("en".into());
        }
        let chapters = md
            .chapters(&id, &langs, self.adult)
            .await
            .map_err(|e| e.to_string())?;
        if chapters.is_empty() {
            return Ok(Vec::new());
        }
        let list = ChapterList {
            ext_id: mangadex::BUILTIN_ID.to_owned(),
            provider: "mangadex".into(),
            lang: None,
            chapters,
        };
        Ok(vec![finish_list(list)])
    }

    /// Every provider's lists for a title: `(ext_id, Ok(lists) | Err(error))`, cached `CHAPTER_TTL_SECS`; a
    /// failed fetch falls back to the stale cache.
    async fn lists(
        &self,
        media_id: &str,
        key: &TitleKey,
        refresh: bool,
    ) -> Vec<(String, Result<Vec<ChapterList>, String>)> {
        let mut set = tokio::task::JoinSet::new();
        let mut ids: Vec<String> = self.exts.iter().map(|(id, _)| id.clone()).collect();
        ids.push(mangadex::BUILTIN_ID.to_owned());
        for ext_id in ids {
            let fresh = if refresh {
                None
            } else {
                self.cached(media_id, &ext_id, CHAPTER_TTL_SECS)
            };
            if let Some(lists) = fresh {
                set.spawn(async move { (ext_id, Ok(lists), false) });
                continue;
            }
            let (ctx, key) = (self.clone(), key.clone());
            set.spawn(async move {
                let r = if ext_id == mangadex::BUILTIN_ID {
                    ctx.fetch_mangadex(&key).await
                } else {
                    ctx.fetch_ext(&ext_id, &key).await
                };
                (ext_id, r, true)
            });
        }
        let mut out = Vec::new();
        while let Some(joined) = set.join_next().await {
            let Ok((ext_id, r, fetched)) = joined else {
                continue;
            };
            let r = match r {
                Ok(lists) => {
                    if fetched {
                        record(&ext_id, true);
                        self.store_lists(media_id, &ext_id, &lists);
                    }
                    Ok(lists)
                }
                Err(e) => {
                    record(&ext_id, false);
                    match self.cached(media_id, &ext_id, i64::MAX) {
                        Some(stale) if !stale.is_empty() => Ok(stale),
                        _ => Err(e),
                    }
                }
            };
            out.push((ext_id, r));
        }
        out.sort_by(|a, b| {
            reliability(&b.0)
                .total_cmp(&reliability(&a.0))
                .then_with(|| a.0.cmp(&b.0))
        });
        out
    }
}

/// Sorted, de-duplicated list with its dominant language (MangaDex lists are built here, not by the parser).
fn finish_list(mut list: ChapterList) -> ChapterList {
    list.chapters.sort_by(|a, b| a.number.total_cmp(&b.number));
    let mut seen = HashSet::new();
    list.chapters.retain(|c| seen.insert(c.chapter_id.clone()));
    let mut counts: HashMap<String, usize> = HashMap::new();
    for c in &list.chapters {
        if let Some(l) = &c.lang {
            *counts.entry(l.clone()).or_default() += 1;
        }
    }
    list.lang = counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(l, _)| l);
    list
}

/// `ChapterProviderVm::ext_id` of a list: the extension, or `<ext_id>/<provider>` for an inner provider.
fn provider_key(list: &ChapterList) -> String {
    if list.provider == list.ext_id || list.provider.is_empty() {
        list.ext_id.clone()
    } else {
        format!("{}/{}", list.ext_id, list.provider)
    }
}

fn provider_name(ext_name: &str, list: &ChapterList) -> String {
    if list.ext_id == mangadex::BUILTIN_ID {
        "MangaDex".to_owned()
    } else if list.provider == list.ext_id || list.provider.is_empty() {
        ext_name.to_owned()
    } else {
        format!("{ext_name} - {}", list.provider)
    }
}

pub async fn chapters(b: &LiveBackend, id: &str, refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
    if super::ext_catalog::is_provider_id(id) {
        return super::ext_catalog::provider_chapters(b, id).await;
    }
    let kind = ReadKindVm::Manga.id();
    let ctx = ChapterCtx::new(b);
    let key = ctx.key_for(id).await?;
    let lists = ctx.lists(id, &key, refresh).await;
    // AniList-first: chapters up to the list progress count as read
    let list_progress = b.store.read_list_entry(kind, id).ok().flatten().map_or(0.0, |r| r.progress as f32);
    let progress: HashMap<(String, String), ReadProgressRow> = b
        .store
        .read_progress(kind, id)
        .map_err(other)?
        .into_iter()
        .map(|r| ((r.ext_id.clone(), r.chapter_id.clone()), r))
        .collect();
    let offline: HashSet<(String, String)> = b
        .store
        .offline_chapters(kind, id)
        .map_err(other)?
        .into_iter()
        .map(|r| (r.ext_id, r.chapter_id))
        .collect();
    let names: HashMap<String, String> = ctx.exts.iter().cloned().collect();
    let mut out = Vec::new();
    for (ext_id, r) in lists {
        let ext_name = names.get(&ext_id).cloned().unwrap_or_else(|| {
            if ext_id == mangadex::BUILTIN_ID {
                "MangaDex".into()
            } else {
                ext_id.clone()
            }
        });
        match r {
            Ok(lists) => {
                for l in lists {
                    let chapters = l
                        .chapters
                        .iter()
                        .map(|c| {
                            let k = (c.ext_id.clone(), c.chapter_id.clone());
                            ChapterVm {
                                reference: ChapterRefVm {
                                    ext_id: c.ext_id.clone(),
                                    chapter_id: c.chapter_id.clone(),
                                },
                                number: c.number,
                                volume: c.volume,
                                title: c.title.clone(),
                                lang: c.lang.clone(),
                                group: c.group.clone(),
                                published_unix: c.published_unix,
                                pages: c.pages,
                                // a chapter covered by the list progress (AniList-first) counts as read
                                read: if c.number > 0.0 && c.number <= list_progress {
                                    1.0
                                } else {
                                    progress
                                        .get(&k)
                                        .map_or(0.0, |p| tk_ui::model::manga::read_fraction(&progress_vm(p)))
                                },
                                downloaded: offline.contains(&k),
                            }
                        })
                        .collect();
                    out.push(ChapterProviderVm {
                        ext_id: provider_key(&l),
                        ext_name: provider_name(&ext_name, &l),
                        lang: l.lang.clone(),
                        chapters,
                        error: None,
                    });
                }
            }
            Err(e) => out.push(ChapterProviderVm {
                ext_id: ext_id.clone(),
                ext_name,
                lang: None,
                chapters: Vec::new(),
                error: Some(UiError::new(ErrorKind::Other, e)),
            }),
        }
    }
    // A MangaDex list that found nothing is noise when an extension answered.
    let any_chapters = out.iter().any(|p| !p.chapters.is_empty());
    out.retain(|p| {
        !(p.ext_id == mangadex::BUILTIN_ID && p.chapters.is_empty() && p.error.is_none() && any_chapters)
    });
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Pages (+ the reader's page IO)
// ---------------------------------------------------------------------------------------------

fn page_vm(p: Page) -> PageVm {
    PageVm {
        index: p.index,
        url: p.url,
        headers: p.headers,
        width: p.width,
        height: p.height,
    }
}

fn image_ext(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    ["jpg", "jpeg", "png", "webp", "gif", "avif"]
        .into_iter()
        .find(|e| lower.ends_with(&format!(".{e}")))
}

/// Page files of an offline chapter, in name order.
fn offline_pages(dir: &Path) -> Vec<PageVm> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .and_then(image_ext)
                            .is_some()
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
        .into_iter()
        .enumerate()
        .map(|(i, p)| PageVm {
            index: i as u32,
            url: format!("{OFFLINE_URL_PREFIX}{}", p.display()),
            headers: Vec::new(),
            width: None,
            height: None,
        })
        .collect()
}

/// Online pages of a chapter (no offline lookup).
async fn fetch_pages(host: &ExtensionHost, chapter: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
    let pages = if chapter.ext_id == MANGADEX_PROVIDER {
        MangaDex::new(client().clone())
            .pages(&chapter.chapter_id)
            .await
            .map_err(prov_err)?
    } else {
        let (provider, inner) = chapter
            .chapter_id
            .split_once(':')
            .unwrap_or(("", chapter.chapter_id.as_str()));
        let params = json!({
            "chapterKey": chapter.chapter_id,
            "provider": if provider.is_empty() { Value::Null } else { Value::from(provider) },
            "providerChapterId": inner,
        });
        let body = tokio::time::timeout(EXT_TIMEOUT, host.call(&chapter.ext_id, "manga/pages", params))
            .await
            .map_err(|_| UiError::new(ErrorKind::Offline, "timeout"))?
            .map_err(other)?;
        parse_ext_pages(&body)
    };
    if pages.is_empty() {
        return Err(UiError::new(ErrorKind::Other, "no pages"));
    }
    Ok(pages.into_iter().map(page_vm).collect())
}

pub async fn pages(b: &LiveBackend, chapter: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
    ensure_page_io(b);
    // Offline first (`manga.download.offline-read`): no network needed.
    if let Ok(Some(row)) =
        b.store
            .offline_chapter(ReadKindVm::Manga.id(), None, &chapter.ext_id, &chapter.chapter_id)
        && let Some(dir) = row.dir.as_deref()
    {
        let list = offline_pages(Path::new(dir));
        if !list.is_empty() {
            return Ok(list);
        }
    }
    if super::ext_catalog::is_provider_chapter(chapter) {
        return super::ext_catalog::provider_pages(b, chapter).await;
    }
    let host = b.ext.host();
    let r = fetch_pages(&host, chapter).await;
    record(&chapter.ext_id, r.is_ok());
    r
}

/// Bytes + MIME of a page: a local file (`file://`) or a remote image fetched with the page headers.
async fn page_bytes(url: String, headers: Vec<(String, String)>) -> Result<(Vec<u8>, String), String> {
    if let Some(path) = url.strip_prefix(OFFLINE_URL_PREFIX) {
        let bytes = tokio::fs::read(path).await.map_err(|e| e.to_string())?;
        let mime = mime_for(path);
        return Ok((bytes, mime));
    }
    let mut req = client().get(&url);
    for (k, v) in &headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let resp = req.send().await.map_err(|e| e.without_url().to_string())?;
    if !resp.status().is_success() {
        return Err(format!("http {}", resp.status().as_u16()));
    }
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.starts_with("image/"))
        .map(str::to_owned)
        .unwrap_or_else(|| mime_for(&url));
    let bytes = resp.bytes().await.map_err(|e| e.without_url().to_string())?;
    Ok((bytes.to_vec(), mime))
}

fn mime_for(name: &str) -> String {
    let clean = name.split(['?', '#']).next().unwrap_or(name);
    match image_ext(clean) {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("avif") => "image/avif",
        _ => "image/jpeg",
    }
    .to_owned()
}

fn ext_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        _ => "jpg",
    }
}

/// The reader's page IO (`tk_ui::components::reader::PageIo`): fetch for the `tkpage` asset handler and "Save
/// page" into `<Pictures>/Tatakai`.
struct LivePageIo {
    pictures: PathBuf,
}

impl PageIo for LivePageIo {
    fn fetch(&self, url: String, headers: Vec<(String, String)>) -> IoFuture<(Vec<u8>, String)> {
        Box::pin(page_bytes(url, headers))
    }

    fn save(&self, url: String, headers: Vec<(String, String)>, file_stem: String) -> IoFuture<String> {
        let dir = self.pictures.clone();
        Box::pin(async move {
            let (bytes, mime) = page_bytes(url, headers).await?;
            tokio::fs::create_dir_all(&dir).await.map_err(|e| e.to_string())?;
            let path = dir.join(format!("{}.{}", safe_name(&file_stem), ext_for_mime(&mime)));
            tokio::fs::write(&path, bytes).await.map_err(|e| e.to_string())?;
            Ok(path.display().to_string())
        })
    }
}

fn ensure_page_io(b: &LiveBackend) {
    static DONE: OnceLock<()> = OnceLock::new();
    DONE.get_or_init(|| {
        let pictures = directories::UserDirs::new()
            .and_then(|u| u.picture_dir().map(Path::to_path_buf))
            .unwrap_or_else(|| b.data_root.join("pictures"))
            .join("Tatakai");
        install_page_io(Arc::new(LivePageIo { pictures }));
    });
}

// ---------------------------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------------------------

pub async fn read_progress(
    b: &LiveBackend,
    kind: ReadKindVm,
    media_id: &str,
) -> UiResult<Vec<ReadProgressVm>> {
    Ok(b.store
        .read_progress(kind.id(), media_id)
        .map_err(other)?
        .iter()
        .map(progress_vm)
        .collect())
}

pub async fn set_read_progress(
    b: &LiveBackend,
    kind: ReadKindVm,
    media_id: &str,
    progress: &ReadProgressVm,
) -> UiResult<()> {
    let info = recall(media_id);
    let row = ReadProgressRow {
        kind: kind.id().to_owned(),
        media_id: media_id.to_owned(),
        ext_id: progress.chapter.ext_id.clone(),
        chapter_id: progress.chapter.chapter_id.clone(),
        chapter_number: progress.chapter_number,
        position: progress.position,
        total: progress.total,
        completed: progress.completed,
        title: info.as_ref().map(|i| i.title.clone()),
        poster: info.as_ref().and_then(|i| i.poster.clone()),
        updated_at: if progress.updated_unix > 0 {
            progress.updated_unix
        } else {
            now()
        },
    };
    b.store.set_read_progress(&row).map_err(other)?;
    // `manga.readlist.anilist-sync`: a finished chapter raises the list progress (never lowers it).
    let chapter = progress.chapter_number.floor();
    if progress.completed && chapter >= 1.0 && settings(b).lists.auto_progress {
        let current = b.store.read_list_entry(kind.id(), media_id).map_err(other)?;
        let done = chapter as u32;
        if current.as_ref().is_none_or(|c| c.progress < done) {
            let status = match current.as_ref().map(|c| c.status.as_str()) {
                None | Some("planning") | Some("paused") => Some(ListStatusVm::Watching),
                _ => None,
            };
            let update = ListEntryUpdateVm {
                status,
                progress: Some(done),
                score: None,
                notes: None,
            };
            if let Err(e) = set_read_list_entry(b, kind, media_id, &update).await {
                tracing::debug!(detail = %e.detail, "automatic manga progress failed");
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Read list (AniList manga list + local mirror + queue)
// ---------------------------------------------------------------------------------------------

fn anilist_token(b: &LiveBackend) -> Option<Token> {
    let json = b.secrets.get(Service::AniList.secret_key()).ok().flatten()?;
    serde_json::from_str::<Token>(&json)
        .ok()
        .filter(|t| t.service == Service::AniList && !t.access_token.is_empty())
}

fn entry_vm(r: &ReadListRow) -> ListEntryVm {
    ListEntryVm {
        media: MediaVm {
            id: r.media_id.clone(),
            title: r.title.clone(),
            cover: r.poster.clone(),
            color: None,
            subtitle: r
                .chapters
                .map(|c| format!("{} / {c}", r.progress))
                .unwrap_or_else(|| r.progress.to_string()),
            score: None,
        },
        service: ListServiceVm::AniList,
        status: ListStatusVm::from_id(&r.status).unwrap_or(ListStatusVm::Planning),
        progress: r.progress,
        episodes_total: r.chapters,
        score: r.score,
        started: None,
        finished: None,
        repeats: 0,
        notes: r.notes.clone(),
        updated_unix: r.updated_at,
        remote_id: r.remote_id,
        pending: r.dirty,
    }
}

/// Novels and manga share the AniList manga list; a title's kind follows its format.
fn kind_of_format(format: Option<&str>) -> ReadKindVm {
    if format == Some("NOVEL") {
        ReadKindVm::Novel
    } else {
        ReadKindVm::Manga
    }
}

/// Pull the AniList manga list into the mirror once per session (dirty rows are kept: their op is queued).
async fn ensure_pulled(b: &LiveBackend) {
    pull_read_lists(b, false).await;
}

/// Mirror the AniList manga list (manga + novels) into `read_list_entries`. Lazily once per run, or every time with
/// `force` ("Sync lists"). Returns how many rows are new or changed (status / progress / score), `None` when nothing
/// was pulled (no account, already pulled, network error).
pub(crate) async fn pull_read_lists(b: &LiveBackend, force: bool) -> Option<u32> {
    static PULLED: OnceLock<tokio::sync::Mutex<bool>> = OnceLock::new();
    let lock = PULLED.get_or_init(|| tokio::sync::Mutex::new(false));
    let mut done = lock.lock().await;
    if *done && !force {
        return None;
    }
    let Some(tok) = anilist_token(b) else {
        *done = true;
        return None;
    };
    let al = anilist();
    let user = match tok.user_id {
        Some(u) => Some(u),
        None => al.viewer(&tok.access_token).await.ok().map(|v| v.id),
    };
    let Some(user) = user else { return None };
    flush(b).await;
    match tokio::time::timeout(Duration::from_secs(15), al.manga_list(&tok.access_token, user)).await {
        Ok(Ok(entries)) => {
            let lang = title_language(&settings(b));
            let mut by_kind: HashMap<&'static str, Vec<ReadListRow>> = HashMap::new();
            for e in entries {
                let Some(al_id) = e.entry.media.media_ref.anilist_id else {
                    continue;
                };
                let kind = kind_of_format(e.format.as_deref());
                by_kind.entry(kind.id()).or_default().push(ReadListRow {
                    kind: kind.id().to_owned(),
                    media_id: al_id.to_string(),
                    anilist_id: Some(al_id),
                    title: display(&e.entry.media, &lang),
                    poster: e.entry.media.cover.clone(),
                    status: anilist_status_to_shared(&e.entry.status)
                        .unwrap_or("planning")
                        .to_owned(),
                    progress: e.entry.progress,
                    chapters: e.chapters,
                    score: e.entry.score_100,
                    notes: e.entry.notes.clone(),
                    remote_id: Some(e.entry.id),
                    updated_at: e.entry.updated_at,
                    dirty: false,
                });
            }
            let mut changed = 0;
            for kind in ReadKindVm::ALL {
                let rows = by_kind.remove(kind.id()).unwrap_or_default();
                let before: HashMap<String, (String, u32, Option<u8>)> = b
                    .store
                    .read_list_entries(kind.id(), None)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|r| (r.media_id, (r.status, r.progress, r.score)))
                    .collect();
                changed += rows
                    .iter()
                    .filter(|r| before.get(&r.media_id) != Some(&(r.status.clone(), r.progress, r.score)))
                    .count() as u32;
                if let Err(e) = b.store.replace_read_list(kind.id(), &rows) {
                    tracing::warn!(error = %e, "could not mirror the AniList manga list");
                }
            }
            *done = true;
            Some(changed)
        }
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "AniList manga list pull failed");
            None
        }
        Err(_) => {
            tracing::warn!("AniList manga list pull timed out");
            None
        }
    }
}

/// Edits go to AniList only with "send changes to accounts" on (`Settings::lists.write_back`), like the anime list.
fn write_back(b: &LiveBackend) -> bool {
    settings(b).lists.write_back
}

/// Push every queued op; network failures stay queued, other failures count attempts.
async fn flush(b: &LiveBackend) {
    let Some(tok) = anilist_token(b) else { return };
    let Ok(queue) = b.store.read_list_queue(None) else {
        return;
    };
    let al = anilist();
    for op in queue {
        let payload: Value = serde_json::from_str(&op.payload).unwrap_or(Value::Null);
        let result: Result<(), ListError> = match op.op.as_str() {
            OP_UPSERT => {
                let Some(al_id) = payload["anilistId"].as_u64().and_then(|v| u32::try_from(v).ok()) else {
                    let _ = b.store.dequeue_read_list_op(op.id);
                    continue;
                };
                let input = EntryInput {
                    status: payload["status"]
                        .as_str()
                        .and_then(shared_status_to_anilist)
                        .map(str::to_owned),
                    progress: payload["progress"].as_u64().and_then(|v| u32::try_from(v).ok()),
                    score_100: payload["score"].as_u64().and_then(|v| u8::try_from(v).ok()),
                    notes: payload["notes"].as_str().map(str::to_owned),
                };
                match al.save_entry(&tok.access_token, al_id, &input).await {
                    Ok(saved) => {
                        if let Ok(Some(mut row)) = b.store.read_list_entry(&op.kind, &op.media_id) {
                            row.remote_id = Some(saved.id);
                            row.dirty = false;
                            let _ = b.store.put_read_list_entry(&row);
                        }
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            OP_DELETE => match payload["remoteId"].as_u64() {
                Some(rid) => al.delete_entry(&tok.access_token, rid).await,
                None => Ok(()),
            },
            _ => Ok(()),
        };
        match result {
            Ok(()) => {
                let _ = b.store.dequeue_read_list_op(op.id);
            }
            Err(ListError::Network(m)) => {
                let _ = b.store.mark_read_list_op_failed(op.id, &m);
                break;
            }
            Err(e) => {
                if op.attempts + 1 >= MAX_ATTEMPTS {
                    let _ = b.store.dequeue_read_list_op(op.id);
                } else {
                    let _ = b.store.mark_read_list_op_failed(op.id, &e.to_string());
                }
            }
        }
    }
}

pub async fn read_list_entry(
    b: &LiveBackend,
    kind: ReadKindVm,
    media_id: &str,
) -> UiResult<Option<ListEntryVm>> {
    ensure_pulled(b).await;
    Ok(b.store
        .read_list_entry(kind.id(), media_id)
        .map_err(other)?
        .as_ref()
        .map(entry_vm))
}

pub async fn set_read_list_entry(
    b: &LiveBackend,
    kind: ReadKindVm,
    media_id: &str,
    update: &ListEntryUpdateVm,
) -> UiResult<ListEntryVm> {
    let existing = b.store.read_list_entry(kind.id(), media_id).map_err(other)?;
    let info = recall(media_id);
    let mut row = existing.unwrap_or_else(|| ReadListRow {
        kind: kind.id().to_owned(),
        media_id: media_id.to_owned(),
        anilist_id: parse_id(media_id).and_then(|i| match i {
            MangaId::AniList(a) => Some(a),
            MangaId::MangaDex(_) => info.as_ref().and_then(|x| x.anilist_id),
        }),
        title: info
            .as_ref()
            .map(|i| i.title.clone())
            .unwrap_or_else(|| media_id.to_owned()),
        poster: info.as_ref().and_then(|i| i.poster.clone()),
        status: ListStatusVm::Planning.id().to_owned(),
        progress: 0,
        chapters: info.as_ref().and_then(|i| i.chapters),
        score: None,
        notes: None,
        remote_id: None,
        updated_at: now(),
        dirty: false,
    });
    if let Some(st) = update.status {
        row.status = st.id().to_owned();
    }
    if let Some(p) = update.progress {
        row.progress = p;
        if row.chapters.is_some_and(|c| c > 0 && p >= c) && update.status.is_none() {
            row.status = ListStatusVm::Completed.id().to_owned();
        }
    }
    if let Some(s) = update.score {
        row.score = (s > 0).then_some(s.min(100));
    }
    if let Some(n) = &update.notes {
        row.notes = Some(n.clone()).filter(|n| !n.trim().is_empty());
    }
    row.updated_at = now();
    let connected = anilist_token(b).is_some() && row.anilist_id.is_some() && write_back(b);
    row.dirty = connected;
    b.store.put_read_list_entry(&row).map_err(other)?;
    if connected {
        let payload = json!({
            "anilistId": row.anilist_id,
            "status": row.status,
            "progress": row.progress,
            "score": row.score,
            "notes": row.notes,
        });
        b.store
            .enqueue_read_list_op(kind.id(), media_id, OP_UPSERT, &payload.to_string(), now())
            .map_err(other)?;
        let _ = tokio::time::timeout(PUSH_BUDGET, flush(b)).await;
    }
    let saved = b
        .store
        .read_list_entry(kind.id(), media_id)
        .map_err(other)?
        .unwrap_or(row);
    Ok(entry_vm(&saved))
}

pub async fn remove_read_list_entry(b: &LiveBackend, kind: ReadKindVm, media_id: &str) -> UiResult<()> {
    let existing = b.store.read_list_entry(kind.id(), media_id).map_err(other)?;
    b.store
        .delete_read_list_entry(kind.id(), media_id)
        .map_err(other)?;
    if let Some(row) = existing
        && anilist_token(b).is_some()
        && write_back(b)
    {
        let payload = json!({ "remoteId": row.remote_id });
        b.store
            .enqueue_read_list_op(kind.id(), media_id, OP_DELETE, &payload.to_string(), now())
            .map_err(other)?;
        let _ = tokio::time::timeout(PUSH_BUDGET, flush(b)).await;
    }
    Ok(())
}

pub async fn read_list_entries(
    b: &LiveBackend,
    kind: ReadKindVm,
    status: Option<ListStatusVm>,
) -> UiResult<Vec<ListEntryVm>> {
    ensure_pulled(b).await;
    Ok(b.store
        .read_list_entries(kind.id(), status.map(ListStatusVm::id))
        .map_err(other)?
        .iter()
        .map(entry_vm)
        .collect())
}

// ---------------------------------------------------------------------------------------------
// Offline chapters
// ---------------------------------------------------------------------------------------------

/// File-system safe component (no separators, no reserved characters, bounded length).
fn safe_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.' | '(' | ')') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned();
    let cut: String = cleaned.chars().take(80).collect();
    if cut.is_empty() {
        "untitled".to_owned()
    } else {
        cut
    }
}

/// Root of the offline manga folders: `<Settings::downloads.dir>/Manga`, else `<Downloads>/Tatakai/Manga`.
fn manga_root(b: &LiveBackend, s: &Settings) -> PathBuf {
    let dir = s.downloads.dir.trim();
    if !dir.is_empty() {
        return PathBuf::from(dir).join("Manga");
    }
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|d| d.join("Tatakai")))
        .unwrap_or_else(|| b.data_root.join("downloads"))
        .join("Manga")
}

/// Everything one background chapter download needs (owned).
struct ChapterJob {
    store: Arc<LocalStore>,
    host: Arc<ExtensionHost>,
    media_id: String,
    title: String,
    poster: Option<String>,
    chapter: ChapterRefVm,
    number: f32,
    dir: PathBuf,
}

impl ChapterJob {
    fn row(
        &self,
        state: &str,
        path: Option<String>,
        bytes: u64,
        error: Option<String>,
        done: bool,
    ) -> ChapterDownloadRow {
        ChapterDownloadRow {
            kind: ReadKindVm::Manga.id().to_owned(),
            media_id: self.media_id.clone(),
            title: self.title.clone(),
            poster: self.poster.clone(),
            chapter: chapter_label(self.number),
            source_id: format!("{}/{}", self.chapter.ext_id, self.chapter.chapter_id),
            state: state.to_owned(),
            path,
            bytes,
            error,
            added_at: now(),
            completed_at: done.then(now),
        }
    }

    async fn run(self) {
        let _ = self
            .store
            .record_chapter_download(&self.row("downloading", None, 0, None, false));
        match self.fetch_all().await {
            Ok((pages, bytes)) => {
                let dir = self.dir.display().to_string();
                let offline = OfflineChapterRow {
                    kind: ReadKindVm::Manga.id().to_owned(),
                    media_id: self.media_id.clone(),
                    ext_id: self.chapter.ext_id.clone(),
                    chapter_id: self.chapter.chapter_id.clone(),
                    dir: Some(dir.clone()),
                    text_json: None,
                    pages: Some(pages),
                    bytes,
                    saved_at: now(),
                };
                if let Err(e) = self.store.set_offline_chapter(&offline) {
                    tracing::warn!(error = %e, "could not register an offline chapter");
                }
                let _ =
                    self.store
                        .record_chapter_download(&self.row("completed", Some(dir), bytes, None, true));
            }
            Err(e) => {
                tracing::warn!(error = %e, chapter = %self.chapter.chapter_id, "chapter download failed");
                let _ = self
                    .store
                    .record_chapter_download(&self.row("failed", None, 0, Some(e), true));
            }
        }
    }

    async fn fetch_all(&self) -> Result<(u32, u64), String> {
        let pages = fetch_pages(&self.host, &self.chapter)
            .await
            .map_err(|e| e.detail)?;
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| e.to_string())?;
        let mut bytes = 0_u64;
        for p in &pages {
            let mut last = String::new();
            let mut saved = false;
            for _attempt in 0..3 {
                match page_bytes(p.url.clone(), p.headers.clone()).await {
                    Ok((data, mime)) => {
                        let path = self
                            .dir
                            .join(format!("{:04}.{}", p.index + 1, ext_for_mime(&mime)));
                        tokio::fs::write(&path, &data).await.map_err(|e| e.to_string())?;
                        bytes += data.len() as u64;
                        saved = true;
                        break;
                    }
                    Err(e) => last = e,
                }
            }
            if !saved {
                return Err(format!("page {}: {last}", p.index + 1));
            }
        }
        Ok((pages.len() as u32, bytes))
    }
}

pub async fn download_chapters(b: &LiveBackend, id: &str, chapters: &[ChapterRefVm]) -> UiResult<u32> {
    if chapters.is_empty() {
        return Ok(0);
    }
    let s = settings(b);
    let info = recall(id);
    let title = info
        .as_ref()
        .map(|i| i.title.clone())
        .unwrap_or_else(|| id.to_owned());
    let poster = info.as_ref().and_then(|i| i.poster.clone());
    let root = manga_root(b, &s).join(safe_name(&title));
    // Chapter numbers for the folder / row labels, from the cached lists.
    let numbers: HashMap<(String, String), f32> = b
        .store
        .chapter_lists(ReadKindVm::Manga.id(), id)
        .map_err(other)?
        .into_iter()
        .filter_map(|(_, j, _)| serde_json::from_str::<Vec<ChapterList>>(&j).ok())
        .flatten()
        .flat_map(|l| {
            l.chapters
                .into_iter()
                .map(|c| ((c.ext_id, c.chapter_id), c.number))
        })
        .collect();
    let host = b.ext.host();
    let mut jobs = Vec::new();
    for c in chapters {
        let number = numbers
            .get(&(c.ext_id.clone(), c.chapter_id.clone()))
            .copied()
            .unwrap_or(0.0);
        let folder = if number > 0.0 {
            format!("Chapter {}", chapter_label(number))
        } else {
            safe_name(&c.chapter_id)
        };
        let job = ChapterJob {
            store: b.store.clone(),
            host: host.clone(),
            media_id: id.to_owned(),
            title: title.clone(),
            poster: poster.clone(),
            chapter: c.clone(),
            number,
            dir: root.join(safe_name(&folder)),
        };
        b.store
            .record_chapter_download(&job.row("queued", None, 0, None, false))
            .map_err(other)?;
        jobs.push(job);
    }
    let n = jobs.len() as u32;
    // One chapter at a time in the background: the call answers as soon as the rows are queued.
    tokio::spawn(async move {
        for job in jobs {
            job.run().await;
        }
    });
    Ok(n)
}

/// Remove the page files this app wrote (never anything else) and the folder when it is left empty.
fn remove_chapter_dir(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .and_then(image_ext)
                    .is_some()
            {
                let _ = std::fs::remove_file(&p);
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
}

pub async fn remove_offline(b: &LiveBackend, kind: ReadKindVm, chapter: &ChapterRefVm) -> UiResult<()> {
    let rows = b
        .store
        .delete_offline_chapter(kind.id(), &chapter.ext_id, &chapter.chapter_id)
        .map_err(other)?;
    for r in &rows {
        if let Some(dir) = r.dir.as_deref() {
            remove_chapter_dir(Path::new(dir));
        }
    }
    b.store
        .delete_chapter_download(kind.id(), &format!("{}/{}", chapter.ext_id, chapter.chapter_id))
        .map_err(other)?;
    Ok(())
}

pub async fn clear_cache(b: &LiveBackend, kind: ReadKindVm, media_id: &str) -> UiResult<()> {
    b.store.clear_chapter_lists(kind.id(), media_id).map_err(other)?;
    if let Ok(mut m) = reliability_map().lock() {
        m.clear();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::manga::Chapter;
    use tk_ui::backend::SearchSortVm;

    #[test]
    fn ids_parse() {
        assert_eq!(parse_id("30013"), Some(MangaId::AniList(30013)));
        assert_eq!(parse_id("md:abc-1"), Some(MangaId::MangaDex("abc-1".into())));
        assert_eq!(parse_id("md:"), None);
        assert_eq!(parse_id("0"), None);
        assert_eq!(parse_id("x"), None);
    }

    #[test]
    fn query_conventions() {
        let q = SearchQueryVm {
            query: "solo".into(),
            season: Some("kr".into()),
            tags: vec![ADULT_ONLY_TAG.into(), "Isekai".into()],
            sort: SearchSortVm::Score,
            page: 0,
            ..SearchQueryVm::default()
        };
        let m = manga_query(&q, 24);
        assert_eq!(m.country.as_deref(), Some("kr"));
        assert_eq!(m.tags, vec!["Isekai".to_owned()]);
        assert_eq!(m.adult, Some(true));
        assert_eq!(m.sort, "score");
        assert_eq!(m.page, 1);
        let both = manga_query(
            &SearchQueryVm {
                adult: true,
                ..SearchQueryVm::default()
            },
            24,
        );
        assert_eq!(both.adult, Some(false));
        assert_eq!(manga_query(&SearchQueryVm::default(), 24).adult, None);
        assert_eq!(home_rows().len(), 7);
    }

    #[test]
    fn subtitles_and_names() {
        assert_eq!(subtitle(true, Some(2020), Some(12), None), "18+ - 2020 - 12 ch");
        assert_eq!(subtitle(false, None, None, Some(3)), "3 vol");
        assert_eq!(safe_name("a/b:c?"), "a_b_c_");
        assert_eq!(safe_name("..."), "untitled");
        assert_eq!(safe_name(&"x".repeat(200)).len(), 80);
        assert_eq!(
            fuzzy((Some(2020), Some(1), Some(5))).as_deref(),
            Some("2020-01-05")
        );
        assert_eq!(fuzzy((None, None, None)), None);
        assert_eq!(status_id(Some("NOT_YET_RELEASED")).as_deref(), Some("upcoming"));
        assert_eq!(mime_for("https://x/a.PNG?x=1"), "image/png");
        assert_eq!(ext_for_mime("image/webp"), "webp");
        assert_eq!(kind_of_format(Some("NOVEL")), ReadKindVm::Novel);
    }

    #[test]
    fn manifests_and_providers() {
        assert!(manifest_reads_manga(
            &json!({ "type": "custom", "capabilities": ["manga"] })
        ));
        assert!(manifest_reads_manga(
            &json!({ "type": "custom", "apiServer": { "routes": ["manga/chapters"] } })
        ));
        assert!(manifest_reads_manga(&json!({ "type": "manga" })));
        assert!(!manifest_reads_manga(
            &json!({ "type": "onlinestream", "capabilities": ["stream"] })
        ));
        let list = ChapterList {
            ext_id: "toko".into(),
            provider: "comick".into(),
            lang: None,
            chapters: Vec::new(),
        };
        assert_eq!(provider_key(&list), "toko/comick");
        assert_eq!(provider_name("Toko", &list), "Toko - comick");
        let own = ChapterList {
            provider: "toko".into(),
            ..list
        };
        assert_eq!(provider_key(&own), "toko");
    }

    #[test]
    fn mangadex_lists_are_finished() {
        let c = |id: &str, n: f32, l: &str| Chapter {
            ext_id: mangadex::BUILTIN_ID.into(),
            chapter_id: id.into(),
            number: n,
            volume: None,
            title: None,
            lang: Some(l.into()),
            group: None,
            published_unix: None,
            pages: None,
        };
        let l = finish_list(ChapterList {
            ext_id: mangadex::BUILTIN_ID.into(),
            provider: "mangadex".into(),
            lang: None,
            chapters: vec![
                c("b", 2.0, "es"),
                c("a", 1.0, "es"),
                c("a", 1.0, "es"),
                c("c", 1.0, "en"),
            ],
        });
        assert_eq!(l.chapters.len(), 3);
        assert_eq!(l.chapters[0].number, 1.0);
        assert_eq!(l.lang.as_deref(), Some("es"));
    }

    #[test]
    fn reliability_is_smoothed() {
        record("test.ext.rel", true);
        record("test.ext.rel", true);
        record("test.ext.rel", false);
        assert!((reliability("test.ext.rel") - 0.6).abs() < 1e-6);
        assert!((reliability("never.seen") - 0.5).abs() < 1e-6);
    }

    #[test]
    fn offline_pages_are_listed_in_order() {
        let dir = std::env::temp_dir().join(format!("tk-manga-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("0002.jpg"), b"b").unwrap();
        std::fs::write(dir.join("0001.png"), b"a").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let pages = offline_pages(&dir);
        assert_eq!(pages.len(), 2);
        assert!(pages[0].url.ends_with("0001.png"));
        assert!(pages[0].url.starts_with(OFFLINE_URL_PREFIX));
        remove_chapter_dir(&dir);
        assert!(dir.join("notes.txt").exists());
        let _ = std::fs::remove_file(dir.join("notes.txt"));
        let _ = std::fs::remove_dir(&dir);
    }
}
