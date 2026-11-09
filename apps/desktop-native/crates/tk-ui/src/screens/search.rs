//! Search: debounced query, stale-response guard (generation counter), paginated results ("Load more" plus
//! infinite scroll), recent searches (6, local, clearable), type chips (anime / characters), origin chips
//! (metadata providers or one extension), the AniList filter sheet (genres, tags with a spoiler switch, format,
//! status, season / year, year / episode / score ranges, order, 18+) applied by the backend query
//! (`Backend::search_filtered`), active filter chips with "Clear", feeds to explore (D6), search by image and
//! master-detail on wide windows.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, MediaVm, SearchPageVm, SearchQueryVm, SearchSortVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, Chip, ChipKind, EmptyState, ErrorState, IconButton, IconButtonSize, Kbd,
    LoadingIndicator, MediaCard, MediaCover, Pill, Sheet, SkeletonGrid, Switch, TextField, Tone, use_uncontrolled_input,
};
use crate::i18n::{lang, lookup, t, tf, tn};
use crate::icons::Icon;
use crate::model::{
    PaneLayout, SEARCH_DEBOUNCE_MS, UiError, format_unix_utc, normalize_query, pane_layout, query_is_searchable,
};
use crate::screens::browse::{HOME_FEEDS, Paged, PagedFooter, Phase, TAG_ADULT, TAG_SPOILER, TAGS_FEED, list_page};
use crate::screens::details::DetailsView;
use crate::shortcuts::Shortcut;

// ---------------------------------------------------------------------------------------------
// Recent searches (pure helpers + web view localStorage)
// ---------------------------------------------------------------------------------------------

pub const HISTORY_MAX: usize = 6;
const HISTORY_KEY: &str = "tk.search.history";

/// Session copy; filled once from localStorage on the first visit.
static HISTORY: GlobalSignal<Vec<String>> = Signal::global(Vec::new);
static HISTORY_LOADED: AtomicBool = AtomicBool::new(false);
/// The page as it was left (query, results, open item, origin, filters): Search is rebuilt on every visit, so
/// coming back from details or the player would otherwise start empty.
static KEPT: GlobalSignal<Option<Kept>> = Signal::global(|| None);
/// Filters another screen asked Search to open with ([`preset_search`]); taken once by the next Search mount.
static PRESET: Mutex<Option<SearchFilters>> = Mutex::new(None);
/// AniList's tag collection (`Backend::feed(TAGS_FEED)`), fetched once per session for the filter sheet.
static TAGS: GlobalSignal<Option<Result<Vec<TagVm>, UiError>>> = Signal::global(|| None);

#[derive(Clone)]
struct Kept {
    query: String,
    results: Paged,
    selected: Option<String>,
    origin: Option<String>,
    filters: SearchFilters,
    kind: SearchKind,
}

/// Open Search with these filters (genre / tag feeds, "Filters" of Browse). Call before navigating to Search.
pub fn preset_search(filters: SearchFilters) {
    if let Ok(mut p) = PRESET.lock() {
        *p = Some(filters);
    }
}

fn take_preset() -> Option<SearchFilters> {
    PRESET.lock().ok().and_then(|mut p| p.take())
}

/// `q` first (normalized), without case-insensitive duplicates, at most [`HISTORY_MAX`]. Queries too
/// short to search are ignored.
pub fn push_history(list: &[String], q: &str) -> Vec<String> {
    let q = normalize_query(q);
    if !query_is_searchable(&q) {
        return list.to_vec();
    }
    let mut out = vec![q.clone()];
    out.extend(list.iter().filter(|x| !x.eq_ignore_ascii_case(&q)).cloned());
    out.truncate(HISTORY_MAX);
    out
}

/// Repair a list read from storage (blank, duplicate or extra entries), keeping its order.
pub fn sanitize_history(list: Vec<String>) -> Vec<String> {
    list.iter().rev().fold(Vec::new(), |acc, q| push_history(&acc, q))
}

fn save_history(list: &[String]) {
    let json = serde_json::to_string(list).unwrap_or_else(|_| "[]".to_owned());
    let lit = serde_json::to_string(&json).unwrap_or_else(|_| "\"[]\"".to_owned());
    let _ = document::eval(&format!("try{{localStorage.setItem('{HISTORY_KEY}',{lit})}}catch(e){{}}"));
}

fn remember(q: &str) {
    let next = push_history(&HISTORY.peek(), q);
    if *HISTORY.peek() != next {
        save_history(&next);
        *HISTORY.write() = next;
    }
}

fn forget(q: &str) {
    let next: Vec<String> = HISTORY.peek().iter().filter(|x| x.as_str() != q).cloned().collect();
    save_history(&next);
    *HISTORY.write() = next;
}

fn clear_history() {
    save_history(&[]);
    *HISTORY.write() = Vec::new();
}

// ---------------------------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------------------------

/// AniList genre names (the values sent to the API); labels are `search.genre.<slug>`.
pub const GENRES: [&str; 18] = [
    "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Horror", "Mahou Shoujo", "Mecha", "Music",
    "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural", "Thriller",
];
/// Offered only with the 18+ switch on.
pub const ADULT_GENRE: &str = "Hentai";
/// AniList `MediaFormat`; labels are `search.format.<lowercase>`.
pub const FORMATS: [&str; 7] = ["TV", "TV_SHORT", "MOVIE", "OVA", "ONA", "SPECIAL", "MUSIC"];
/// `SearchQueryVm::status` values; labels are `search.status.<id>`.
pub const STATUSES: [&str; 5] = ["releasing", "finished", "upcoming", "cancelled", "hiatus"];
/// `SearchQueryVm::season` values; labels are `search.season.<id>`.
pub const SEASONS: [&str; 4] = ["winter", "spring", "summer", "fall"];
const YEARS_SHOWN: i32 = 8;
/// Tags listed per category before "Show all".
const TAGS_PER_GROUP: usize = 40;

/// i18n key suffix of a genre (`Slice of Life` -> `slice_of_life`).
pub fn genre_slug(genre: &str) -> String {
    genre.trim().to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// Translated genre label; unknown genres are shown as AniList names them.
pub fn genre_label(genre: &str) -> String {
    let key = format!("search.genre.{}", genre_slug(genre));
    let text = t(&key);
    if text == key { genre.to_owned() } else { text }
}

/// Everything the filter sheet sets. `Default` = no filter (relevance order, 18+ hidden).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchFilters {
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    /// One of [`FORMATS`].
    pub format: Option<String>,
    /// One of [`STATUSES`].
    pub status: Option<String>,
    /// One of [`SEASONS`].
    pub season: Option<String>,
    /// Season year with `season`, else the start year.
    pub year: Option<i32>,
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    pub episodes_min: Option<u32>,
    pub episodes_max: Option<u32>,
    /// 0..=100
    pub score_min: Option<u32>,
    pub score_max: Option<u32>,
    pub sort: SearchSortVm,
    pub adult: bool,
}

/// One active filter, shown as a dismissible chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterChip {
    Genre(String),
    Tag(String),
    Format,
    Status,
    Season,
    Year,
    YearRange,
    Episodes,
    Score,
    Sort,
    Adult,
}

fn toggle_in(list: &[String], value: &str) -> Vec<String> {
    if list.iter().any(|x| x == value) {
        list.iter().filter(|x| *x != value).cloned().collect()
    } else {
        let mut v = list.to_vec();
        v.push(value.to_owned());
        v
    }
}

fn toggle_opt(slot: &Option<String>, value: &str) -> Option<String> {
    if slot.as_deref() == Some(value) { None } else { Some(value.to_owned()) }
}

fn range_text<T: std::fmt::Display>(min: Option<T>, max: Option<T>) -> String {
    let a = min.map(|v| v.to_string()).unwrap_or_default();
    let b = max.map(|v| v.to_string()).unwrap_or_default();
    format!("{a}-{b}")
}

fn ordered<T: PartialOrd + Copy>(a: Option<T>, b: Option<T>) -> (Option<T>, Option<T>) {
    match (a, b) {
        (Some(x), Some(y)) if x > y => (Some(y), Some(x)),
        other => other,
    }
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Reversed ranges swapped, scores capped at 100, duplicate genres / tags dropped.
    pub fn sanitized(mut self) -> Self {
        (self.year_from, self.year_to) = ordered(self.year_from, self.year_to);
        (self.episodes_min, self.episodes_max) = ordered(self.episodes_min, self.episodes_max);
        self.score_min = self.score_min.map(|s| s.min(100));
        self.score_max = self.score_max.map(|s| s.min(100));
        (self.score_min, self.score_max) = ordered(self.score_min, self.score_max);
        let dedup = |v: Vec<String>| v.into_iter().fold(Vec::<String>::new(), |mut acc, x| {
            if !x.trim().is_empty() && !acc.contains(&x) {
                acc.push(x);
            }
            acc
        });
        self.genres = dedup(self.genres);
        self.tags = dedup(self.tags);
        if !self.adult {
            self.genres.retain(|g| g != ADULT_GENRE);
        }
        self
    }

    /// Active filters in sheet order.
    pub fn active(&self) -> Vec<FilterChip> {
        let mut v: Vec<FilterChip> = Vec::new();
        if self.sort != SearchSortVm::Relevance {
            v.push(FilterChip::Sort);
        }
        v.extend(self.genres.iter().cloned().map(FilterChip::Genre));
        v.extend(self.tags.iter().cloned().map(FilterChip::Tag));
        if self.format.is_some() {
            v.push(FilterChip::Format);
        }
        if self.status.is_some() {
            v.push(FilterChip::Status);
        }
        if self.season.is_some() {
            v.push(FilterChip::Season);
        }
        if self.year.is_some() {
            v.push(FilterChip::Year);
        }
        if self.year_from.is_some() || self.year_to.is_some() {
            v.push(FilterChip::YearRange);
        }
        if self.episodes_min.is_some() || self.episodes_max.is_some() {
            v.push(FilterChip::Episodes);
        }
        if self.score_min.is_some() || self.score_max.is_some() {
            v.push(FilterChip::Score);
        }
        if self.adult {
            v.push(FilterChip::Adult);
        }
        v
    }

    /// These filters without `chip`.
    pub fn without(&self, chip: &FilterChip) -> Self {
        let mut n = self.clone();
        match chip {
            FilterChip::Genre(g) => n.genres.retain(|x| x != g),
            FilterChip::Tag(g) => n.tags.retain(|x| x != g),
            FilterChip::Format => n.format = None,
            FilterChip::Status => n.status = None,
            FilterChip::Season => n.season = None,
            FilterChip::Year => n.year = None,
            FilterChip::YearRange => (n.year_from, n.year_to) = (None, None),
            FilterChip::Episodes => (n.episodes_min, n.episodes_max) = (None, None),
            FilterChip::Score => (n.score_min, n.score_max) = (None, None),
            FilterChip::Sort => n.sort = SearchSortVm::Relevance,
            FilterChip::Adult => {
                n.adult = false;
                n.genres.retain(|g| g != ADULT_GENRE);
            }
        }
        n
    }

    /// The backend query for `text` (ignored when too short to search) and `page`.
    pub fn query(&self, text: &str, page: u32) -> SearchQueryVm {
        let text = normalize_query(text);
        SearchQueryVm {
            query: if query_is_searchable(&text) { text } else { String::new() },
            genres: self.genres.clone(),
            tags: self.tags.clone(),
            format: self.format.clone(),
            status: self.status.clone(),
            season: self.season.clone(),
            year: self.year,
            year_from: self.year_from,
            year_to: self.year_to,
            episodes_min: self.episodes_min,
            episodes_max: self.episodes_max,
            score_min: self.score_min,
            score_max: self.score_max,
            sort: self.sort,
            adult: self.adult,
            page: page.max(1),
        }
    }

    /// Stable text form for the page's `data-filters` (`genre=Action;season=winter;year=2025`).
    pub fn signature(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.sort != SearchSortVm::Relevance {
            parts.push(format!("sort={}", self.sort.id()));
        }
        parts.extend(self.genres.iter().map(|g| format!("genre={g}")));
        parts.extend(self.tags.iter().map(|g| format!("tag={g}")));
        if let Some(f) = &self.format {
            parts.push(format!("format={f}"));
        }
        if let Some(s) = &self.status {
            parts.push(format!("status={s}"));
        }
        if let Some(s) = &self.season {
            parts.push(format!("season={s}"));
        }
        if let Some(y) = self.year {
            parts.push(format!("year={y}"));
        }
        if self.year_from.is_some() || self.year_to.is_some() {
            parts.push(format!("years={}", range_text(self.year_from, self.year_to)));
        }
        if self.episodes_min.is_some() || self.episodes_max.is_some() {
            parts.push(format!("episodes={}", range_text(self.episodes_min, self.episodes_max)));
        }
        if self.score_min.is_some() || self.score_max.is_some() {
            parts.push(format!("score={}", range_text(self.score_min, self.score_max)));
        }
        if self.adult {
            parts.push("adult=1".to_owned());
        }
        parts.join(";")
    }
}

fn chip_label(f: &SearchFilters, chip: &FilterChip) -> String {
    match chip {
        FilterChip::Genre(g) => genre_label(g),
        FilterChip::Tag(g) => tf("search.chip.tag", &[("name", g)]),
        FilterChip::Format => f.format.as_deref().map(format_label).unwrap_or_default(),
        FilterChip::Status => f.status.as_deref().map(|s| t(&format!("search.status.{s}"))).unwrap_or_default(),
        FilterChip::Season => f.season.as_deref().map(|s| t(&format!("search.season.{s}"))).unwrap_or_default(),
        FilterChip::Year => f.year.map(|y| y.to_string()).unwrap_or_default(),
        FilterChip::YearRange => tf("search.chip.years", &[("range", &range_text(f.year_from, f.year_to))]),
        FilterChip::Episodes => tf("search.chip.episodes", &[("range", &range_text(f.episodes_min, f.episodes_max))]),
        FilterChip::Score => tf("search.chip.score", &[("range", &range_text(f.score_min, f.score_max))]),
        FilterChip::Sort => tf("search.chip.sort", &[("sort", &t(&format!("search.sort.{}", f.sort.id())))]),
        FilterChip::Adult => t("search.adult.chip"),
    }
}

fn format_label(format: &str) -> String {
    t(&format!("search.format.{}", format.to_ascii_lowercase()))
}

fn chip_key(chip: &FilterChip) -> String {
    match chip {
        FilterChip::Genre(g) => format!("g-{g}"),
        FilterChip::Tag(g) => format!("t-{g}"),
        other => format!("{other:?}"),
    }
}

fn current_year() -> i32 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format_unix_utc(now).get(..4).and_then(|y| y.parse().ok()).unwrap_or(2025)
}

/// A range bound typed in the sheet (`""` = open).
pub fn parse_bound(s: &str) -> Option<u32> {
    s.trim().parse::<u32>().ok()
}

/// A year typed in the sheet; nonsense years are ignored.
pub fn parse_year(s: &str) -> Option<i32> {
    s.trim().parse::<i32>().ok().filter(|y| (1900..=2100).contains(y))
}

/// `85` -> `8.5`.
fn score_text(score: u32) -> String {
    format!("{:.1}", f64::from(score) / 10.0)
}

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

/// What the result list shows: titles or AniList characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchKind {
    #[default]
    Anime,
    Characters,
}

/// One search, paged by [`fetch`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchRequest {
    /// `Backend::search_filtered` (page set per request).
    Metadata(SearchQueryVm),
    /// `Backend::search_in_extension`.
    Extension { ext: String, query: String },
    /// `Backend::feed("characters:<query>")`.
    Characters(String),
}

impl SearchRequest {
    /// `data-origin` of the result list.
    pub fn origin(&self) -> String {
        match self {
            Self::Metadata(_) => "metadata".to_owned(),
            Self::Extension { ext, .. } => format!("ext:{ext}"),
            Self::Characters(_) => "characters".to_owned(),
        }
    }
}

/// The request for the current inputs, `None` when there is nothing to search (Search then shows recent searches
/// and feeds). Metadata searches run with a query or with filters alone; extensions and characters need a query.
pub fn request_for(kind: SearchKind, origin: Option<&str>, query: &str, filters: &SearchFilters) -> Option<SearchRequest> {
    let q = normalize_query(query);
    let searchable = query_is_searchable(&q);
    match (kind, origin) {
        (SearchKind::Characters, _) => searchable.then(|| SearchRequest::Characters(q)),
        (SearchKind::Anime, Some(ext)) => searchable.then(|| SearchRequest::Extension { ext: ext.to_owned(), query: q }),
        (SearchKind::Anime, None) => {
            (searchable || !filters.is_empty()).then(|| SearchRequest::Metadata(filters.query(&q, 1)))
        }
    }
}

async fn fetch(backend: BackendRef, req: SearchRequest, page: u32) -> UiResult<SearchPageVm> {
    match req {
        SearchRequest::Metadata(mut q) => {
            q.page = page.max(1);
            backend.search_filtered(&q).await
        }
        SearchRequest::Extension { ext, query } => {
            backend.search_in_extension(&ext, &query, page).await.map(|items| list_page(items, page))
        }
        SearchRequest::Characters(q) => backend.feed(&format!("characters:{q}"), page).await,
    }
}

/// The selected id if it is still in `items`, else the first result (wide layout selects one).
fn effective_selection(items: &[MediaVm], selected: Option<String>) -> Option<String> {
    selected.filter(|s| items.iter().any(|m| m.id == *s)).or_else(|| items.first().map(|m| m.id.clone()))
}

/// One tag of the filter sheet (from the `anilist-tags` feed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagVm {
    pub name: String,
    pub category: String,
    pub spoiler: bool,
    pub adult: bool,
}

/// Decode the `anilist-tags` feed items (see `screens::browse` docs).
pub fn tags_from_items(items: &[MediaVm]) -> Vec<TagVm> {
    items
        .iter()
        .map(|m| {
            let flags = m.score.unwrap_or(0);
            TagVm {
                name: m.title.clone(),
                category: m.subtitle.clone(),
                spoiler: flags & TAG_SPOILER != 0,
                adult: flags & TAG_ADULT != 0,
            }
        })
        .collect()
}

/// Tags the sheet lists: spoilers only with the spoiler switch, 18+ only with the 18+ switch, matching `filter`
/// (case-insensitive), grouped by category in order.
pub fn visible_tags(tags: &[TagVm], filter: &str, spoilers: bool, adult: bool) -> Vec<(String, Vec<TagVm>)> {
    let needle = filter.trim().to_lowercase();
    let mut groups: Vec<(String, Vec<TagVm>)> = Vec::new();
    for tag in tags {
        if (tag.spoiler && !spoilers) || (tag.adult && !adult) {
            continue;
        }
        if !needle.is_empty() && !tag.name.to_lowercase().contains(&needle) {
            continue;
        }
        match groups.iter_mut().find(|(c, _)| *c == tag.category) {
            Some((_, list)) => list.push(tag.clone()),
            None => groups.push((tag.category.clone(), vec![tag.clone()])),
        }
    }
    groups
}

// ---------------------------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------------------------

#[component]
pub fn SearchPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let backend = services.backend.clone();

    // A preset (genre / tag feed) starts a fresh page; otherwise the page comes back as it was left.
    let preset = use_hook(take_preset);
    let kept = if preset.is_some() { None } else { KEPT.peek().clone() };
    // A request still running when the page was left has no task to finish it: it runs again below.
    let rerun = preset.is_some()
        || kept.as_ref().is_some_and(|k| matches!(k.results.phase, Phase::Loading) || k.results.more_loading);
    let mut query = use_signal(|| kept.as_ref().map(|k| k.query.clone()).unwrap_or_default());
    let mut results = use_signal(|| kept.as_ref().map(|k| k.results.clone()).unwrap_or_default());
    let mut selected = use_signal(|| kept.as_ref().and_then(|k| k.selected.clone()));
    // `None` = metadata providers; `Some(ext id)` = search through that extension.
    let mut origin = use_signal(|| kept.as_ref().and_then(|k| k.origin.clone()));
    let mut filters = use_signal(|| preset.clone().or_else(|| kept.as_ref().map(|k| k.filters.clone())).unwrap_or_default());
    let mut kind = use_signal(|| kept.as_ref().map(|k| k.kind).unwrap_or_default());
    // The request whose pages `results` holds (load more reuses it).
    let mut active = use_signal(|| {
        kept.as_ref().and_then(|k| request_for(k.kind, k.origin.as_deref(), &k.query, &k.filters))
    });
    use_drop(move || {
        *KEPT.write() = Some(Kept {
            query: query.peek().clone(),
            results: results.peek().clone(),
            selected: selected.peek().clone(),
            origin: origin.peek().clone(),
            filters: filters.peek().clone(),
            kind: *kind.peek(),
        });
    });
    let mut filters_open = use_signal(|| false);

    let exts = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });

    use_future(|| async move {
        if HISTORY_LOADED.swap(true, Ordering::Relaxed) {
            return;
        }
        let js = format!("try{{return JSON.parse(localStorage.getItem('{HISTORY_KEY}')||'[]')}}catch(e){{return []}}");
        if let Ok(list) = document::eval(&js).join::<Vec<String>>().await
            && HISTORY.peek().is_empty()
        {
            *HISTORY.write() = sanitize_history(list);
        }
    });

    // Ctrl+K while already on this page: focus the field. (Autofocus covers arriving here.)
    let mut seen = use_signal(|| ui.shortcut.peek().as_ref().map_or(0, |e| e.seq));
    use_effect(move || {
        let Some(evt) = (ui.shortcut)() else { return };
        if *seen.peek() != evt.seq {
            seen.set(evt.seq);
            if evt.action == Shortcut::FocusSearch {
                let _ = document::eval("const i=document.getElementById('tk-search-input');if(i){i.focus();i.select();}");
            }
        }
    });

    // (Re)start the search for the current inputs; `debounce` while typing.
    let run = use_callback({
        let backend = backend.clone();
        move |debounce: bool| {
            let req = request_for(*kind.peek(), origin.peek().as_deref(), &query.peek(), &filters.peek());
            let Some(req) = req else {
                let next = results.peek().idle();
                results.set(next);
                active.set(None);
                return;
            };
            // Bump the generation now (older answers are dropped) but keep the current items while debouncing.
            let generation = results.peek().generation + 1;
            let bumped = Paged { generation, more_loading: false, ..results.peek().clone() };
            results.set(bumped);
            let backend = backend.clone();
            spawn(async move {
                if debounce {
                    tokio::time::sleep(Duration::from_millis(SEARCH_DEBOUNCE_MS)).await;
                    if results.peek().generation != generation {
                        return;
                    }
                }
                results.set(Paged { generation, phase: Phase::Loading, ..Paged::default() });
                active.set(Some(req.clone()));
                let result = fetch(backend, req, 1).await;
                let next = results.peek().finish(generation, 1, result);
                results.set(next);
            });
        }
    });

    let load_more = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let page = results.peek().next_page();
            let req = active.peek().clone();
            let (Some(page), Some(req)) = (page, req) else { return };
            let next = results.peek().with_more_loading();
            let generation = next.generation;
            results.set(next);
            let backend = backend.clone();
            spawn(async move {
                let result = fetch(backend, req, page).await;
                let next = results.peek().finish(generation, page, result);
                results.set(next);
            });
        }
    });

    use_hook(move || {
        if rerun {
            run.call(false);
        }
    });

    let open_item = use_callback(move |id: String| {
        remember(&query.peek());
        if *kind.peek() == SearchKind::Characters {
            navigator.push(Route::CharacterPage { id });
        } else {
            navigator.push(Route::DetailsPage { id });
        }
    });

    let dual = pane_layout((ui.width)()) == PaneLayout::Dual;
    let query_text = query.read().clone();
    let (input_generation, record_input) = use_uncontrolled_input(&query_text);
    let placeholder = if kind() == SearchKind::Characters { t("search.placeholder.characters") } else { t("search.placeholder") };
    let hint = t("search.hint");
    let clear_label = t("search.clear");
    let type_label = t("search.type");
    let active_filters = filters.read().clone();
    let current_kind = kind();
    let by_extension = origin.read().is_some();
    let metadata_view = current_kind == SearchKind::Anime && !by_extension;
    let page_state = results.read().clone();
    let request = active.read().clone();
    let origin_attr = request.as_ref().map(SearchRequest::origin).unwrap_or_else(|| "none".to_owned());

    // Enabled, loadable extensions become origin chips.
    let ext_chips: Vec<(String, String)> = match &*exts.read() {
        Some(Ok(list)) => list
            .iter()
            .filter(|e| e.enabled && e.compat.can_enable())
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect(),
        _ => Vec::new(),
    };

    let detail_id = if metadata_view && page_state.phase == Phase::Ready {
        effective_selection(&page_state.items, selected())
    } else {
        None
    };
    let split = dual && detail_id.is_some();
    let signature = active_filters.signature();

    let chips = rsx! {
        div {
            class: "tk-chips tk-search__chips",
            role: "group",
            "aria-label": "{type_label}",
            "data-filters": "{signature}",
            "data-kind": if current_kind == SearchKind::Characters { "characters" } else { "anime" },
            Chip {
                label: t("type.anime"),
                on: current_kind == SearchKind::Anime,
                onclick: move |_| {
                    if *kind.peek() != SearchKind::Anime {
                        kind.set(SearchKind::Anime);
                        run.call(false);
                    }
                },
            }
            Chip {
                label: t("search.type.characters"),
                icon: "user".to_string(),
                on: current_kind == SearchKind::Characters,
                onclick: move |_| {
                    if *kind.peek() != SearchKind::Characters {
                        kind.set(SearchKind::Characters);
                        selected.set(None);
                        run.call(false);
                    }
                },
            }
            // Wave B (l, `manga.route`): the manga home opens its Search tab with the current query.
            Chip {
                label: t("manga.title"),
                icon: "grid".to_string(),
                onclick: move |_| crate::screens::manga::open_search(navigator, &query.peek()),
            }
            // Wave B (m, `novel.route`): the novel home takes the pending query once when it mounts.
            Chip {
                label: t("novel.search.type"),
                icon: "bookmark".to_string(),
                onclick: move |_| {
                    crate::screens::novel::home::set_pending_query(&query.peek());
                    navigator.push(Route::NovelHomePage {});
                },
            }
            if current_kind == SearchKind::Anime {
                span { class: "tk-search__sep", "aria-hidden": "true" }
                Chip {
                    label: t("source.metadata"),
                    icon: "sync".to_string(),
                    on: !by_extension,
                    onclick: move |_| {
                        if origin.peek().is_some() {
                            origin.set(None);
                            run.call(false);
                        }
                    },
                }
                for (ext_id , name) in ext_chips {
                    {
                        let on = origin.read().as_deref() == Some(ext_id.as_str());
                        let pick = ext_id.clone();
                        rsx! {
                            Chip {
                                key: "{ext_id}",
                                label: name.clone(),
                                icon: "extensions".to_string(),
                                on,
                                onclick: move |_| {
                                    if origin.peek().as_deref() != Some(pick.as_str()) {
                                        origin.set(Some(pick.clone()));
                                        selected.set(None);
                                        run.call(false);
                                    }
                                },
                            }
                        }
                    }
                }
            }
            if metadata_view {
                for chip in active_filters.active() {
                    {
                        let label = chip_label(&active_filters, &chip);
                        let key = chip_key(&chip);
                        rsx! {
                            Chip {
                                key: "{key}",
                                label,
                                on: true,
                                dismiss_label: t("common.remove"),
                                on_dismiss: move |_| {
                                    let next = filters.peek().without(&chip);
                                    filters.set(next);
                                    run.call(false);
                                },
                            }
                        }
                    }
                }
                if !active_filters.is_empty() {
                    Button {
                        label: t("search.clear_filters"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        icon: "close".to_string(),
                        onclick: move |_| {
                            filters.set(SearchFilters::default());
                            run.call(false);
                        },
                    }
                }
            }
        }
    };

    let results_view = match (&request, &page_state.phase) {
        (_, Phase::Idle) | (None, _) => {
            let history = HISTORY.read().clone();
            let (title, body) = if current_kind == SearchKind::Characters {
                (t("search.empty.characters.title"), t("search.empty.characters.body"))
            } else if by_extension {
                (t("search.empty.ext.title"), t("search.empty.ext.body"))
            } else {
                (t("search.empty.title"), t("search.empty.body"))
            };
            rsx! {
                EmptyState { icon: "search".to_string(), title, body }
                if !history.is_empty() {
                    section { class: "tk-section",
                        div { class: "tk-section__head",
                            h2 { class: "tk-section__title tk-section__title--sm tk-muted", {t("search.history")} }
                            Button {
                                label: t("search.history.clear"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                icon: "trash".to_string(),
                                onclick: move |_| clear_history(),
                            }
                        }
                        div { class: "tk-search__history", "data-history": "{history.len()}",
                            for q in history {
                                {
                                    let (again, gone) = (q.clone(), q.clone());
                                    rsx! {
                                        div { key: "{q}", class: "tk-search__hist",
                                            button {
                                                class: "tk-search__histq",
                                                r#type: "button",
                                                onclick: move |_| {
                                                    query.set(again.clone());
                                                    run.call(false);
                                                },
                                                Icon { name: "history".to_string(), size: 20 }
                                                span { "{q}" }
                                            }
                                            IconButton {
                                                icon: "close".to_string(),
                                                label: t("common.remove"),
                                                size: IconButtonSize::Sm,
                                                onclick: move |_| forget(&gone),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if current_kind == SearchKind::Anime && !by_extension {
                    ExploreFeeds {}
                }
            }
        }
        (_, Phase::Loading) => rsx! {
            SkeletonGrid { count: 12 }
        },
        (_, Phase::Failed(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| run.call(false) }
        },
        (Some(req), Phase::Ready) => {
            let items = page_state.items.clone();
            if items.is_empty() {
                let has_filters = metadata_view && !active_filters.is_empty();
                rsx! {
                    div { "data-results": "true", "data-page": "{page_state.page}", "data-origin": "{origin_attr}",
                        EmptyState {
                            icon: "search".to_string(),
                            title: t("search.no_results.title"),
                            body: if query_text.trim().is_empty() { t("search.no_results.filters") } else { tf("search.no_results.body", &[("query", &query_text)]) },
                            action_label: if has_filters { t("search.clear_filters") } else { String::new() },
                            on_action: move |_| {
                                filters.set(SearchFilters::default());
                                run.call(false);
                            },
                        }
                    }
                }
            } else {
                // Wave B (b, `settings.privacy.mature-content`): 18+ results are blurred in the Blur mode.
                let card_blurred = crate::model::home::blurred(&ui.settings.read().home, active_filters.adult);
                let shown = page_state.total.map(|n| n as usize).unwrap_or(items.len());
                let count = tn("search.results", shown);
                let sel = detail_id.clone();
                let list_mode = dual && matches!(req, SearchRequest::Metadata(_));
                rsx! {
                    p { class: "tk-search__count tk-muted", role: "status", "{count}" }
                    if list_mode {
                        div {
                            class: "tk-list tk-search__results",
                            role: "listbox",
                            "aria-label": "{count}",
                            "data-results": "true",
                            "data-page": "{page_state.page}",
                            "data-origin": "{origin_attr}",
                            for m in items {
                                {
                                    let is_sel = sel.as_deref() == Some(m.id.as_str());
                                    let (id, open_id) = (m.id.clone(), m.id.clone());
                                    rsx! {
                                        ResultRow {
                                            key: "{m.id}",
                                            media: m.clone(),
                                            selected: is_sel,
                                            onclick: move |_| selected.set(Some(id.clone())),
                                            on_open: move |_| open_item.call(open_id.clone()),
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        div {
                            class: "tk-grid",
                            "data-results": "true",
                            "data-page": "{page_state.page}",
                            "data-origin": "{origin_attr}",
                            for m in items {
                                {
                                    let id = m.id.clone();
                                    rsx! {
                                        div { key: "{m.id}", class: "tk-blur-wrap", "data-blurred": "{card_blurred}",
                                            MediaCard { media: m.clone(), onclick: move |_| open_item.call(id.clone()) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    PagedFooter {
                        page: page_state.page,
                        has_next: page_state.has_next,
                        loading: page_state.more_loading,
                        error: page_state.more_error.clone(),
                        on_more: move |_| load_more.call(()),
                    }
                }
            }
        }
    };

    rsx! {
        div { class: "tk-page tk-page--search tk-search", "data-dual": "{split}",
            div { class: "tk-search__list",
                div { class: "tk-page__head",
                    h1 { class: "tk-page__title", {t("nav.search")} }
                    Button {
                        label: t("imgsearch.open"),
                        icon: "camera".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::ImageSearchPage {});
                        },
                    }
                }
                div { class: "tk-searchbar", role: "search",
                    Icon { name: "search".to_string(), size: 22 }
                    // A one-item keyed list so clearing remounts the uncontrolled input (see use_uncontrolled_input).
                    {std::iter::once(input_generation).map(|input_generation| {
                        let record_input = record_input.clone();
                        rsx! {
                            input {
                                key: "{input_generation}",
                                id: "tk-search-input",
                                class: "tk-searchbar__input",
                                r#type: "search",
                                placeholder: "{placeholder}",
                                "aria-label": "{placeholder}",
                                title: "{hint}",
                                autofocus: true,
                                autocomplete: "off",
                                spellcheck: "false",
                                initial_value: "{query_text}",
                                // autofocus only applies on page load; a remount after clearing keeps the focus here.
                                onmounted: move |e| {
                                    if input_generation > 0 {
                                        spawn(async move {
                                            let _ = e.set_focus(true).await;
                                        });
                                    }
                                },
                                oninput: move |e| {
                                    record_input(&e.value());
                                    query.set(e.value());
                                    run.call(true);
                                },
                                onkeydown: move |e| {
                                    if e.key() == Key::Enter {
                                        remember(&query.peek());
                                    }
                                },
                            }
                        }
                    })}
                    Kbd { keys: "Ctrl K".to_string() }
                    if !query_text.is_empty() {
                        IconButton {
                            icon: "close".to_string(),
                            label: clear_label.clone(),
                            onclick: move |_| {
                                query.set(String::new());
                                selected.set(None);
                                run.call(false);
                            },
                        }
                    }
                    if metadata_view {
                        IconButton {
                            icon: "tune".to_string(),
                            label: t("search.filters"),
                            toggled: !active_filters.is_empty(),
                            onclick: move |_| filters_open.set(true),
                        }
                    }
                }
                {chips}
                {results_view}
            }
            if split {
                aside { class: "tk-search__detail",
                    if let Some(sel) = detail_id.clone() {
                        DetailsView { key: "{sel}", id: sel.clone(), embedded: true }
                    }
                }
            }
            if filters_open() {
                FilterSheet {
                    value: active_filters.clone(),
                    on_apply: move |f: SearchFilters| {
                        filters.set(f.sanitized());
                        filters_open.set(false);
                        selected.set(None);
                        run.call(false);
                    },
                    on_close: move |_| filters_open.set(false),
                }
            }
        }
    }
}

/// Feeds and genres to explore when nothing is searched (plan 14 D6: collections, trending and genres are feeds
/// of Search).
#[component]
fn ExploreFeeds() -> Element {
    let navigator = use_navigator();
    rsx! {
        section { class: "tk-section tk-search__explore", "data-explore": "true",
            h2 { class: "tk-section__title tk-section__title--sm tk-muted", {t("search.explore")} }
            div { class: "tk-chips",
                for f in HOME_FEEDS {
                    Chip {
                        key: "{f}",
                        kind: ChipKind::Assist,
                        label: t(&format!("feed.{f}")),
                        onclick: move |_| {
                            navigator.push(Route::BrowsePage { feed: f.to_owned() });
                        },
                    }
                }
            }
            h2 { class: "tk-section__title tk-section__title--sm tk-muted", {t("search.explore.genres")} }
            div { class: "tk-chips",
                for g in GENRES {
                    Chip {
                        key: "{g}",
                        kind: ChipKind::Assist,
                        label: genre_label(g),
                        onclick: move |_| {
                            navigator.push(Route::BrowsePage { feed: format!("genre:{g}") });
                        },
                    }
                }
            }
        }
    }
}

/// One row of the wide result list (cover, title, subtitle, score). Double click or Enter on the
/// selected row opens Details.
#[component]
fn ResultRow(
    media: MediaVm,
    selected: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    #[props(default)] on_open: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            class: "tk-result",
            r#type: "button",
            role: "option",
            "aria-selected": "{selected}",
            onclick: move |e| onclick.call(e),
            ondoubleclick: move |_| on_open.call(()),
            onkeydown: move |e| {
                if e.key() == Key::Enter && selected {
                    e.prevent_default();
                    on_open.call(());
                }
            },
            div { class: "tk-result__cover",
                MediaCover { src: media.cover.clone(), color: media.color.clone(), title: media.title.clone() }
            }
            div { class: "tk-result__body",
                div { class: "tk-result__title", "{media.title}" }
                if !media.subtitle.is_empty() {
                    div { class: "tk-result__sub", "{media.subtitle}" }
                }
            }
            div { class: "tk-result__meta",
                if let Some(s) = media.score {
                    Pill { text: score_text(s), tone: Tone::Primary, plain: true }
                }
            }
        }
    }
}

/// Side sheet with every AniList filter. Edits a draft; Apply commits it, Reset commits the empty set.
#[component]
fn FilterSheet(value: SearchFilters, on_apply: EventHandler<SearchFilters>, on_close: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let mut draft = use_signal(|| value.clone());
    let opt = |v: Option<String>| v.unwrap_or_default();
    let mut year_from = use_signal(|| opt(value.year_from.map(|v| v.to_string())));
    let mut year_to = use_signal(|| opt(value.year_to.map(|v| v.to_string())));
    let mut eps_min = use_signal(|| opt(value.episodes_min.map(|v| v.to_string())));
    let mut eps_max = use_signal(|| opt(value.episodes_max.map(|v| v.to_string())));
    let mut score_min = use_signal(|| opt(value.score_min.map(|v| v.to_string())));
    let mut score_max = use_signal(|| opt(value.score_max.map(|v| v.to_string())));
    let mut tag_filter = use_signal(String::new);
    let mut spoilers = use_signal(|| false);
    let mut show_all_tags = use_signal(|| false);

    let load_tags = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            *TAGS.write() = None;
            let backend = backend.clone();
            spawn(async move {
                let r = backend.feed(TAGS_FEED, 1).await.map(|p| tags_from_items(&p.items));
                *TAGS.write() = Some(r);
            });
        }
    });
    use_hook(move || {
        let missing = !matches!(&*TAGS.peek(), Some(Ok(_)));
        if missing {
            load_tags.call(());
        }
    });

    let collect = move || {
        let mut d = draft.peek().clone();
        d.year_from = parse_year(&year_from.peek());
        d.year_to = parse_year(&year_to.peek());
        d.episodes_min = parse_bound(&eps_min.peek());
        d.episodes_max = parse_bound(&eps_max.peek());
        d.score_min = parse_bound(&score_min.peek());
        d.score_max = parse_bound(&score_max.peek());
        d.sanitized()
    };

    let year = current_year();
    let years: Vec<i32> = (0..YEARS_SHOWN).map(|i| year + 1 - i).collect();
    let current = draft.read().clone();
    let mut genres: Vec<&'static str> = GENRES.to_vec();
    if current.adult {
        genres.push(ADULT_GENRE);
    }
    let tag_state = TAGS.read().clone();
    let filter_text = tag_filter.read().clone();
    let show_spoilers = spoilers();

    rsx! {
        Sheet {
            title: t("search.filters"),
            wide: true,
            on_close: move |_| on_close.call(()),
            footer: rsx! {
                Button {
                    label: t("search.reset"),
                    variant: ButtonVariant::Text,
                    onclick: move |_| on_apply.call(SearchFilters::default()),
                }
                Button { label: t("search.apply"), onclick: move |_| on_apply.call(collect()) }
            },
            div { class: "tk-filters", "data-filter-sheet": "true",
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.sort\")}",
                    h3 { class: "tk-filters__label", {t("search.sort")} }
                    div { class: "tk-chips",
                        for s in SearchSortVm::ALL {
                            Chip {
                                key: "{s.id()}",
                                label: t(&format!("search.sort.{}", s.id())),
                                on: current.sort == s,
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.sort = s;
                                    draft.set(d);
                                },
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.genre\")}",
                    h3 { class: "tk-filters__label", {t("search.genre")} }
                    div { class: "tk-chips",
                        for g in genres {
                            Chip {
                                key: "{g}",
                                label: genre_label(g),
                                on: current.genres.iter().any(|x| x == g),
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.genres = toggle_in(&d.genres, g);
                                    draft.set(d);
                                },
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.format\")}",
                    h3 { class: "tk-filters__label", {t("search.format")} }
                    div { class: "tk-chips",
                        for f in FORMATS {
                            Chip {
                                key: "{f}",
                                label: format_label(f),
                                on: current.format.as_deref() == Some(f),
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.format = toggle_opt(&d.format, f);
                                    draft.set(d);
                                },
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.status\")}",
                    h3 { class: "tk-filters__label", {t("search.status")} }
                    div { class: "tk-chips",
                        for s in STATUSES {
                            Chip {
                                key: "{s}",
                                label: t(&format!("search.status.{s}")),
                                on: current.status.as_deref() == Some(s),
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.status = toggle_opt(&d.status, s);
                                    draft.set(d);
                                },
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.season\")}",
                    h3 { class: "tk-filters__label", {t("search.season")} }
                    div { class: "tk-chips",
                        for s in SEASONS {
                            Chip {
                                key: "{s}",
                                label: t(&format!("search.season.{s}")),
                                on: current.season.as_deref() == Some(s),
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.season = toggle_opt(&d.season, s);
                                    draft.set(d);
                                },
                            }
                        }
                    }
                    div { class: "tk-chips", role: "group", "aria-label": "{t(\"search.year\")}",
                        for y in years {
                            Chip {
                                key: "{y}",
                                label: y.to_string(),
                                on: current.year == Some(y),
                                onclick: move |_| {
                                    let mut d = draft.peek().clone();
                                    d.year = if d.year == Some(y) { None } else { Some(y) };
                                    draft.set(d);
                                },
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.range.years\")}",
                    h3 { class: "tk-filters__label", {t("search.range.years")} }
                    div { class: "tk-filters__range",
                        TextField { label: t("search.range.from"), kind: "number".to_string(), value: year_from(), placeholder: "2010".to_string(), oninput: move |v: String| year_from.set(v) }
                        TextField { label: t("search.range.to"), kind: "number".to_string(), value: year_to(), placeholder: year.to_string(), oninput: move |v: String| year_to.set(v) }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.range.episodes\")}",
                    h3 { class: "tk-filters__label", {t("search.range.episodes")} }
                    div { class: "tk-filters__range",
                        TextField { label: t("search.range.min"), kind: "number".to_string(), value: eps_min(), placeholder: "1".to_string(), oninput: move |v: String| eps_min.set(v) }
                        TextField { label: t("search.range.max"), kind: "number".to_string(), value: eps_max(), placeholder: "26".to_string(), oninput: move |v: String| eps_max.set(v) }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.range.score\")}",
                    h3 { class: "tk-filters__label", {t("search.range.score")} }
                    div { class: "tk-filters__range",
                        TextField { label: t("search.range.min"), kind: "number".to_string(), value: score_min(), placeholder: "0".to_string(), hint: t("search.range.score_hint"), oninput: move |v: String| score_min.set(v) }
                        TextField { label: t("search.range.max"), kind: "number".to_string(), value: score_max(), placeholder: "100".to_string(), oninput: move |v: String| score_max.set(v) }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.tags\")}", "data-tags": "{current.tags.len()}",
                    h3 { class: "tk-filters__label", {t("search.tags")} }
                    if !current.tags.is_empty() {
                        div { class: "tk-chips",
                            for g in current.tags.clone() {
                                {
                                    let name = g.clone();
                                    rsx! {
                                        Chip {
                                            key: "sel-{g}",
                                            label: g.clone(),
                                            on: true,
                                            dismiss_label: t("common.remove"),
                                            on_dismiss: move |_| {
                                                let mut d = draft.peek().clone();
                                                d.tags.retain(|x| *x != name);
                                                draft.set(d);
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                    div { class: "tk-filters__tagbar",
                        TextField {
                            label: t("search.tags.find"),
                            value: filter_text.clone(),
                            icon: "search".to_string(),
                            oninput: move |v: String| tag_filter.set(v),
                        }
                        label { class: "tk-filters__switch", "data-spoilers": "{show_spoilers}",
                            Switch { checked: show_spoilers, label: t("search.tags.spoilers"), on_toggle: move |v: bool| spoilers.set(v) }
                            span { {t("search.tags.spoilers")} }
                        }
                    }
                    match tag_state {
                        None => rsx! {
                            LoadingIndicator { small: true }
                        },
                        Some(Err(e)) => rsx! {
                            ErrorState { error: e, compact: true, on_retry: move |_| load_tags.call(()) }
                        },
                        Some(Ok(all)) => {
                            let groups = visible_tags(&all, &filter_text, show_spoilers, current.adult);
                            let open = !filter_text.trim().is_empty();
                            let limit = if show_all_tags() || open { usize::MAX } else { TAGS_PER_GROUP };
                            if groups.is_empty() {
                                rsx! {
                                    p { class: "tk-muted", {t("search.tags.none")} }
                                }
                            } else {
                                rsx! {
                                    div { class: "tk-filters__tags",
                                        for (category , list) in groups {
                                            details { key: "{category}", class: "tk-filters__tagcat", open,
                                                summary { "{category} · {list.len()}" }
                                                div { class: "tk-chips",
                                                    for tag in list.into_iter().take(limit) {
                                                        {
                                                            let name = tag.name.clone();
                                                            let on = current.tags.iter().any(|x| *x == tag.name);
                                                            rsx! {
                                                                Chip {
                                                                    key: "{tag.name}",
                                                                    label: tag.name.clone(),
                                                                    on,
                                                                    onclick: move |_| {
                                                                        let mut d = draft.peek().clone();
                                                                        d.tags = toggle_in(&d.tags, &name);
                                                                        draft.set(d);
                                                                    },
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if !open && !show_all_tags() {
                                            Button {
                                                label: t("search.tags.all"),
                                                variant: ButtonVariant::Text,
                                                size: ButtonSize::Sm,
                                                onclick: move |_| show_all_tags.set(true),
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "tk-filters__group", role: "group", "aria-label": "{t(\"search.adult\")}",
                    label { class: "tk-filters__switch", "data-adult": "{current.adult}",
                        Switch {
                            checked: current.adult,
                            label: t("search.adult"),
                            on_toggle: move |v: bool| {
                                let mut d = draft.peek().clone();
                                d.adult = v;
                                if !v {
                                    d.genres.retain(|g| g != ADULT_GENRE);
                                }
                                draft.set(d);
                            },
                        }
                        span { {t("search.adult")} }
                    }
                    p { class: "tk-filters__hint tk-muted", {t("search.adult.hint")} }
                }
            }
        }
    }
}

/// Short month name for character birthdays (`character.month.<1..=12>`), in the current UI language. Usable
/// outside a component (tk-app builds `CharacterVm::birthday` with it through `screens::character`).
pub fn month_name(month: u32) -> Option<String> {
    (1..=12).contains(&month).then(|| lookup(lang(), &format!("character.month.{month}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str) -> MediaVm {
        MediaVm { id: id.into(), title: id.into(), cover: None, color: None, subtitle: String::new(), score: None }
    }

    #[test]
    fn history_is_newest_first_deduped_and_capped() {
        let mut h: Vec<String> = Vec::new();
        for q in ["frieren", "dandadan", "Frieren", "a", "  one   piece ", "b1", "b2", "b3", "b4"] {
            h = push_history(&h, q);
        }
        assert_eq!(h.len(), HISTORY_MAX);
        assert_eq!(h[0], "b4");
        assert!(h.contains(&"one piece".to_owned()));
        assert!(!h.iter().any(|x| x == "a"));
        assert_eq!(h.iter().filter(|x| x.eq_ignore_ascii_case("frieren")).count(), 1);
        assert_eq!(sanitize_history(vec!["x1".into(), "X1".into(), " ".into(), "y2".into()]), vec!["x1", "y2"]);
    }

    #[test]
    fn filters_build_the_backend_query() {
        let f = SearchFilters {
            genres: vec!["Action".into()],
            season: Some("winter".into()),
            year: Some(2025),
            episodes_min: Some(1),
            episodes_max: Some(13),
            score_min: Some(80),
            sort: SearchSortVm::Score,
            ..SearchFilters::default()
        };
        let q = f.query("  x ", 3);
        assert_eq!(q.query, "", "too short to search: filters only");
        assert_eq!(q.genres, vec!["Action".to_owned()]);
        assert_eq!(q.season.as_deref(), Some("winter"));
        assert_eq!(q.year, Some(2025));
        assert_eq!(q.page, 3);
        assert_eq!(q.sort, SearchSortVm::Score);
        assert_eq!(f.query("frieren", 0).query, "frieren");
        assert_eq!(f.query("frieren", 0).page, 1);
        let sig = f.signature();
        assert!(sig.contains("genre=Action"), "{sig}");
        assert!(sig.contains("season=winter;year=2025"));
        assert!(sig.contains("episodes=1-13"));
        assert!(sig.contains("score=80-"));
        assert!(sig.starts_with("sort=score"));
    }

    #[test]
    fn chips_remove_one_filter_each() {
        let f = SearchFilters {
            genres: vec!["Action".into(), "Drama".into()],
            tags: vec!["Isekai".into()],
            format: Some("TV".into()),
            year_from: Some(2010),
            adult: true,
            ..SearchFilters::default()
        };
        let chips = f.active();
        assert_eq!(chips.len(), 6);
        let g = f.without(&FilterChip::Genre("Action".into()));
        assert_eq!(g.genres, vec!["Drama".to_owned()]);
        let all = chips.iter().fold(f.clone(), |acc, c| acc.without(c));
        assert!(all.is_empty());
        assert!(SearchFilters::default().active().is_empty());
        assert_eq!(toggle_in(&["a".into()], "a"), Vec::<String>::new());
        assert_eq!(toggle_opt(&Some("TV".into()), "TV"), None);
        assert_eq!(toggle_opt(&None, "TV").as_deref(), Some("TV"));
    }

    #[test]
    fn sanitize_orders_ranges_and_drops_hidden_genres() {
        let f = SearchFilters {
            year_from: Some(2015),
            year_to: Some(2010),
            score_min: Some(140),
            score_max: Some(50),
            genres: vec!["Hentai".into(), "Action".into(), "Action".into()],
            ..SearchFilters::default()
        }
        .sanitized();
        assert_eq!((f.year_from, f.year_to), (Some(2010), Some(2015)));
        assert_eq!((f.score_min, f.score_max), (Some(50), Some(100)));
        assert_eq!(f.genres, vec!["Action".to_owned()]);
        assert_eq!(parse_bound(" 13 "), Some(13));
        assert_eq!(parse_bound("-1"), None);
        assert_eq!(parse_year("1850"), None);
        assert_eq!(parse_year("2010"), Some(2010));
    }

    #[test]
    fn requests_follow_kind_origin_and_inputs() {
        let none = SearchFilters::default();
        assert_eq!(request_for(SearchKind::Anime, None, "a", &none), None);
        let filtered = SearchFilters { genres: vec!["Action".into()], ..SearchFilters::default() };
        match request_for(SearchKind::Anime, None, "", &filtered) {
            Some(SearchRequest::Metadata(q)) => assert_eq!(q.genres, vec!["Action".to_owned()]),
            other => panic!("{other:?}"),
        }
        let ext = request_for(SearchKind::Anime, Some("tatakai.extension.toko"), " one  piece", &filtered).unwrap();
        assert_eq!(ext, SearchRequest::Extension { ext: "tatakai.extension.toko".into(), query: "one piece".into() });
        assert_eq!(ext.origin(), "ext:tatakai.extension.toko");
        assert_eq!(request_for(SearchKind::Anime, Some("x"), "", &filtered), None);
        assert_eq!(request_for(SearchKind::Characters, None, "luffy", &none), Some(SearchRequest::Characters("luffy".into())));
        assert_eq!(request_for(SearchKind::Characters, None, "l", &filtered), None);
    }

    #[test]
    fn tags_decode_and_filter() {
        let tag = |name: &str, cat: &str, flags: u32| MediaVm { subtitle: cat.into(), score: Some(flags), ..m(name) };
        let tags = tags_from_items(&[tag("Isekai", "Setting", 0), tag("Twist", "Theme", TAG_SPOILER), tag("Nudity", "Sexual", TAG_ADULT), tag("Space", "Setting", 0)]);
        assert!(tags[1].spoiler && tags[2].adult);
        let g = visible_tags(&tags, "", false, false);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].1.len(), 2);
        assert_eq!(visible_tags(&tags, "", true, true).len(), 3);
        let g = visible_tags(&tags, "ISE", false, false);
        assert_eq!(g[0].1[0].name, "Isekai");
        assert_eq!(g[0].1.len(), 1);
    }

    #[test]
    fn genre_slugs_and_selection() {
        assert_eq!(genre_slug("Slice of Life"), "slice_of_life");
        assert_eq!(genre_slug("Sci-Fi"), "sci_fi");
        let items = [m("a"), m("b")];
        assert_eq!(effective_selection(&items, Some("b".into())).as_deref(), Some("b"));
        assert_eq!(effective_selection(&items, Some("zz".into())).as_deref(), Some("a"));
        assert_eq!(effective_selection(&[], None), None);
        assert_eq!(score_text(85), "8.5");
        assert_eq!(month_name(13), None);
    }
}
