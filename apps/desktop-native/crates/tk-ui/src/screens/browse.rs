//! "See all" of a home row and the other feeds of Search (wave A, worker **a**; plan 14 D6): the feed's pages
//! (`Backend::feed(feed, page)`) as an infinite grid. Route `/browse/:feed`.
//!
//! Feed ids ([`FeedId`], shared with tk-app's `live/search.rs`, which parses them with [`FeedId::parse`]):
//! - a home row: the suffix of `SectionVm::title_key` (`trending`, `airing`, `seasonal`, `popular`, `top_rated`,
//!   `upcoming`, `recently_completed`);
//! - `trending:day|week|month|all`: trending over a time window (AniList `mediaTrends`; `all` = all-time popularity);
//! - `genre:<AniList genre>` / `tag:<AniList tag>`: the catalogue of one genre / tag (the page offers the filters
//!   of Search for it);
//! - `characters:<query>`: AniList characters (Search's "Characters" type). Items are characters as cards:
//!   `MediaVm::id` = AniList character id, `title` = name, `cover` = portrait, `subtitle` = native name;
//! - `anilist-tags`: AniList's tag collection for the tag filter (never browsed). Items: `id` = `title` = tag name,
//!   `subtitle` = category, `score` = [`TAG_SPOILER`] / [`TAG_ADULT`] bit flags.
//!
//! [`Paged`] (pure, tested) is the paginated state Search, Browse and Studio share; [`PagedGrid`] renders a
//! [`PageSource`] with it, and [`PagedFooter`] is the "Load more" / infinite-scroll sentinel.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{MediaVm, SearchPageVm, UiResult};
use crate::components::{Button, ButtonVariant, EmptyState, ErrorState, LoadingIndicator, MediaCard, SegOption, Segmented, SkeletonGrid};
use crate::i18n::{t, tf, tn};
use crate::model::UiError;
use crate::screens::search::{SearchFilters, genre_label, preset_search};

/// Home row feeds, in home order plus the two collections the home does not show.
pub const HOME_FEEDS: [&str; 7] = ["trending", "airing", "seasonal", "popular", "top_rated", "upcoming", "recently_completed"];
/// Feed id of AniList's tag collection (`Backend::feed`).
pub const TAGS_FEED: &str = "anilist-tags";
/// `MediaVm::score` bit of a tag item: AniList `isGeneralSpoiler`.
pub const TAG_SPOILER: u32 = 1;
/// `MediaVm::score` bit of a tag item: AniList `isAdult`.
pub const TAG_ADULT: u32 = 2;

/// Time window of the trending feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrendWindow {
    Day,
    Week,
    Month,
    /// All-time (most popular ever).
    All,
}

impl TrendWindow {
    pub const ALL: [TrendWindow; 4] = [Self::Day, Self::Week, Self::Month, Self::All];

    pub fn id(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::All => "all",
        }
    }

    /// Window length in seconds (`None` = no window).
    pub fn seconds(self) -> Option<i64> {
        match self {
            Self::Day => Some(86_400),
            Self::Week => Some(7 * 86_400),
            Self::Month => Some(30 * 86_400),
            Self::All => None,
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.id() == id)
    }
}

/// A feed of Search / Browse (see the module docs for the string forms).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedId {
    /// One of [`HOME_FEEDS`].
    Home(String),
    Trending(TrendWindow),
    Genre(String),
    Tag(String),
    Characters(String),
    Tags,
}

impl FeedId {
    pub fn parse(s: &str) -> Option<FeedId> {
        let s = s.trim();
        if s == TAGS_FEED {
            return Some(Self::Tags);
        }
        if HOME_FEEDS.contains(&s) {
            return Some(Self::Home(s.to_owned()));
        }
        let (kind, value) = s.split_once(':')?;
        let value = value.trim();
        match kind {
            "trending" => TrendWindow::from_id(value).map(Self::Trending),
            "genre" if !value.is_empty() => Some(Self::Genre(value.to_owned())),
            "tag" if !value.is_empty() => Some(Self::Tag(value.to_owned())),
            "characters" if !value.is_empty() => Some(Self::Characters(value.to_owned())),
            _ => None,
        }
    }

    /// The string form (`parse(id()) == Some(self)`).
    pub fn id(&self) -> String {
        match self {
            Self::Home(f) => f.clone(),
            Self::Trending(w) => format!("trending:{}", w.id()),
            Self::Genre(g) => format!("genre:{g}"),
            Self::Tag(g) => format!("tag:{g}"),
            Self::Characters(q) => format!("characters:{q}"),
            Self::Tags => TAGS_FEED.to_owned(),
        }
    }

    /// Feeds whose items are characters, not titles.
    pub fn is_characters(&self) -> bool {
        matches!(self, Self::Characters(_))
    }
}

// ---------------------------------------------------------------------------------------------
// Paginated state
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Phase {
    /// Nothing requested (Search without a query or filters).
    Idle,
    Loading,
    Ready,
    Failed(UiError),
}

/// Paginated results. `generation` drops stale answers: every new request bumps it and only an answer of the
/// current generation is applied.
#[derive(Debug, Clone, PartialEq)]
pub struct Paged {
    pub generation: u64,
    pub phase: Phase,
    pub items: Vec<MediaVm>,
    /// Last page applied (0 = none).
    pub page: u32,
    pub has_next: bool,
    pub total: Option<u32>,
    pub more_loading: bool,
    pub more_error: Option<UiError>,
}

impl Default for Paged {
    fn default() -> Self {
        Self { generation: 0, phase: Phase::Idle, items: Vec::new(), page: 0, has_next: false, total: None, more_loading: false, more_error: None }
    }
}

impl Paged {
    /// A new generation with nothing to show.
    pub fn idle(&self) -> Self {
        Self { generation: self.generation + 1, ..Self::default() }
    }

    /// A new generation loading its first page.
    pub fn loading(&self) -> Self {
        Self { generation: self.generation + 1, phase: Phase::Loading, ..Self::default() }
    }

    /// The next page to request, when one can be requested now.
    pub fn next_page(&self) -> Option<u32> {
        (self.phase == Phase::Ready && self.has_next && !self.more_loading).then_some(self.page + 1)
    }

    pub fn with_more_loading(&self) -> Self {
        Self { more_loading: true, more_error: None, ..self.clone() }
    }

    /// Apply the answer for `page` of `generation`. Later pages append the items not already shown; a page that
    /// adds nothing ends the pagination (protects against providers that repeat their last page).
    pub fn finish(&self, generation: u64, page: u32, result: UiResult<SearchPageVm>) -> Self {
        if generation != self.generation {
            return self.clone();
        }
        let mut n = self.clone();
        n.more_loading = false;
        match result {
            Ok(p) if page <= 1 => {
                let mut items: Vec<MediaVm> = Vec::with_capacity(p.items.len());
                for m in p.items {
                    if !items.iter().any(|x| x.id == m.id) {
                        items.push(m);
                    }
                }
                n.items = items;
                n.page = 1;
                n.has_next = p.has_next;
                n.total = p.total;
                n.phase = Phase::Ready;
                n.more_error = None;
            }
            Ok(p) => {
                let before = n.items.len();
                for m in p.items {
                    if !n.items.iter().any(|x| x.id == m.id) {
                        n.items.push(m);
                    }
                }
                n.page = page;
                n.has_next = p.has_next && n.items.len() > before;
                if p.total.is_some() {
                    n.total = p.total;
                }
                n.more_error = None;
            }
            Err(e) if page <= 1 => {
                n.phase = Phase::Failed(e);
                n.items.clear();
                n.page = 0;
                n.has_next = false;
            }
            Err(e) => n.more_error = Some(e),
        }
        n
    }
}

/// Wrap a list answer without page info (`search_in_extension`) as a page: there is a next page while the
/// provider keeps returning something.
pub fn list_page(items: Vec<MediaVm>, page: u32) -> SearchPageVm {
    let has_next = !items.is_empty();
    SearchPageVm { items, page, has_next, total: None }
}

/// "Load more" button plus an infinite-scroll sentinel (re-armed on every page), or the inline error of the last
/// page request with its retry.
#[component]
pub fn PagedFooter(page: u32, has_next: bool, loading: bool, error: Option<UiError>, on_more: EventHandler<()>) -> Element {
    if let Some(e) = error {
        return rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| on_more.call(()) }
        };
    }
    if !has_next {
        return rsx! {};
    }
    rsx! {
        // Keyed by page so each page mounts a fresh observer (an element that stays visible fires only once).
        {std::iter::once(page).map(|p| {
            let next = p + 1;
            rsx! {
            div {
                key: "{p}",
                class: "tk-paged__more",
                "data-load-more": "{next}",
                onvisible: move |e| {
                    if e.is_intersecting().unwrap_or(false) && !loading {
                        on_more.call(());
                    }
                },
                if loading {
                    LoadingIndicator { small: true }
                } else {
                    Button { label: t("search.load_more"), variant: ButtonVariant::Tonal, onclick: move |_| on_more.call(()) }
                }
            }
            }
        })}
    }
}

/// What a [`PagedGrid`] pages through.
#[derive(Debug, Clone, PartialEq)]
pub enum PageSource {
    /// `Backend::feed`.
    Feed(String),
    /// `Backend::search_by_studio`.
    Studio(String),
}

/// A paginated grid of cards for `source`; `on_open` receives the card id. Key the component by its source: it
/// loads once per mount.
#[component]
pub fn PagedGrid(
    source: PageSource,
    origin: String,
    on_open: EventHandler<String>,
    /// Empty state texts (default: `browse.empty.*`).
    #[props(default)] empty_title: String,
    #[props(default)] empty_body: String,
) -> Element {
    let services = use_context::<Services>();
    let mut paged = use_signal(Paged::default);

    let load = use_callback({
        let backend = services.backend.clone();
        let source = source.clone();
        move |page: u32| {
            let next = if page <= 1 { paged.peek().loading() } else { paged.peek().with_more_loading() };
            let generation = next.generation;
            paged.set(next);
            let backend = backend.clone();
            let source = source.clone();
            spawn(async move {
                let result = match &source {
                    PageSource::Feed(f) => backend.feed(f, page).await,
                    PageSource::Studio(s) => backend.search_by_studio(s, page).await,
                };
                let next = paged.peek().finish(generation, page, result);
                paged.set(next);
            });
        }
    });
    use_hook(move || load.call(1));

    let p = paged.read().clone();
    let more = move |_: ()| {
        // Read first: `load` writes the signal, which must not be borrowed meanwhile.
        let next = paged.peek().next_page();
        if let Some(n) = next {
            load.call(n);
        }
    };
    match p.phase {
        Phase::Idle | Phase::Loading => rsx! {
            SkeletonGrid { count: 18 }
        },
        Phase::Failed(e) => rsx! {
            ErrorState { error: e, on_retry: move |_| load.call(1) }
        },
        Phase::Ready if p.items.is_empty() => rsx! {
            EmptyState {
                icon: "search".to_string(),
                title: if empty_title.is_empty() { t("browse.empty.title") } else { empty_title.clone() },
                body: if empty_body.is_empty() { t("browse.empty.body") } else { empty_body.clone() },
            }
        },
        Phase::Ready => {
            let count = match p.total {
                Some(total) => tn("search.results", total as usize),
                None => tn("search.results", p.items.len()),
            };
            rsx! {
                p { class: "tk-search__count tk-muted", role: "status", "{count}" }
                div {
                    class: "tk-grid",
                    "data-results": "true",
                    "data-page": "{p.page}",
                    "data-origin": "{origin}",
                    "data-has-next": "{p.has_next}",
                    for m in p.items.clone() {
                        {
                            let id = m.id.clone();
                            rsx! {
                                MediaCard { key: "{m.id}", media: m.clone(), onclick: move |_| on_open.call(id.clone()) }
                            }
                        }
                    }
                }
                PagedFooter { page: p.page, has_next: p.has_next, loading: p.more_loading, error: p.more_error.clone(), on_more: more }
            }
        }
    }
}

fn feed_title(id: &FeedId) -> String {
    match id {
        FeedId::Home(f) => t(&format!("feed.{f}")),
        FeedId::Trending(_) => t("feed.trending"),
        FeedId::Genre(g) => tf("browse.genre", &[("name", &genre_label(g))]),
        FeedId::Tag(g) => tf("browse.tag", &[("name", g)]),
        FeedId::Characters(q) => tf("browse.characters", &[("query", q)]),
        FeedId::Tags => t("browse.title"),
    }
}

#[component]
pub fn BrowsePage(feed: String) -> Element {
    let navigator = use_navigator();
    let Some(id) = FeedId::parse(&feed).filter(|f| *f != FeedId::Tags) else {
        return rsx! {
            div { class: "tk-page tk-page--browse",
                div { class: "tk-page__head",
                    h1 { class: "tk-page__title", {t("browse.title")} }
                }
                EmptyState {
                    icon: "info".to_string(),
                    title: t("browse.unknown.title"),
                    body: tf("browse.unknown.body", &[("feed", &feed)]),
                    action_label: t("browse.to_search"),
                    on_action: move |_| {
                        navigator.push(Route::SearchPage {});
                    },
                }
            }
        };
    };
    let title = feed_title(&id);
    let characters = id.is_characters();
    // Trending: "now" (the home row) plus the four windows.
    let is_trending = matches!(&id, FeedId::Trending(_)) || id == FeedId::Home("trending".into());
    let windows: Option<Vec<SegOption>> = is_trending.then(|| {
        let mut v: Vec<SegOption> = vec![("trending".to_owned(), t("browse.window.now"))];
        v.extend(TrendWindow::ALL.into_iter().map(|w| (format!("trending:{}", w.id()), t(&format!("browse.window.{}", w.id())))));
        v
    });
    // Genre / tag feeds open Search with that filter, where every other filter applies too.
    let preset: Option<SearchFilters> = match &id {
        FeedId::Genre(g) => Some(SearchFilters { genres: vec![g.clone()], ..SearchFilters::default() }),
        FeedId::Tag(g) => Some(SearchFilters { tags: vec![g.clone()], ..SearchFilters::default() }),
        _ => None,
    };
    let feed_key = id.id();
    rsx! {
        div { class: "tk-page tk-page--browse", "data-feed": "{feed_key}",
            div { class: "tk-page__head tk-browse__head",
                h1 { class: "tk-page__title", "{title}" }
                if let Some(f) = preset.clone() {
                    Button {
                        label: t("search.filters"),
                        icon: "tune".to_string(),
                        variant: ButtonVariant::Tonal,
                        onclick: move |_| {
                            preset_search(f.clone());
                            navigator.push(Route::SearchPage {});
                        },
                    }
                }
            }
            if let Some(options) = windows {
                div { class: "tk-browse__windows", "data-trend-window": "{feed_key}",
                    Segmented {
                        options,
                        selected: feed_key.clone(),
                        label: t("browse.window"),
                        on_select: move |next: String| {
                            navigator.replace(Route::BrowsePage { feed: next });
                        },
                    }
                }
            }
            // A one-item keyed list: switching feeds remounts the grid (it loads once per mount).
            {std::iter::once(feed_key.clone()).map(|key| {
                let source = PageSource::Feed(key.clone());
                let origin = format!("feed:{key}");
                rsx! {
                    PagedGrid {
                        key: "{key}",
                        source,
                        origin,
                        on_open: move |item: String| {
                            if characters {
                                navigator.push(Route::CharacterPage { id: item });
                            } else {
                                navigator.push(Route::DetailsPage { id: item });
                            }
                        },
                    }
                }
            })}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ErrorKind;

    fn m(id: &str) -> MediaVm {
        MediaVm { id: id.into(), title: id.into(), cover: None, color: None, subtitle: String::new(), score: None }
    }

    fn page(ids: &[&str], has_next: bool) -> UiResult<SearchPageVm> {
        Ok(SearchPageVm { items: ids.iter().map(|i| m(i)).collect(), page: 1, has_next, total: Some(40) })
    }

    #[test]
    fn feed_ids_round_trip() {
        for s in ["trending", "top_rated", "trending:week", "genre:Slice of Life", "tag:Isekai", "characters:luffy", TAGS_FEED] {
            let id = FeedId::parse(s).unwrap();
            assert_eq!(id.id(), s);
        }
        assert_eq!(FeedId::parse("trending:year"), None);
        assert_eq!(FeedId::parse("genre:"), None);
        assert_eq!(FeedId::parse("nope"), None);
        assert!(FeedId::parse("characters:x").unwrap().is_characters());
        assert_eq!(TrendWindow::Week.seconds(), Some(604_800));
        assert_eq!(TrendWindow::All.seconds(), None);
    }

    #[test]
    fn pages_append_without_duplicates_and_stop() {
        let p = Paged::default().loading();
        let g = p.generation;
        let p = p.finish(g, 1, page(&["a", "b", "a"], true));
        assert_eq!(p.phase, Phase::Ready);
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.next_page(), Some(2));
        let p = p.with_more_loading();
        assert_eq!(p.next_page(), None);
        let p = p.finish(g, 2, page(&["b", "c"], true));
        assert_eq!(p.items.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        assert_eq!(p.page, 2);
        assert!(p.has_next);
        // A page that adds nothing ends the pagination even if the provider says there is more.
        let p = p.with_more_loading().finish(g, 3, page(&["a"], true));
        assert!(!p.has_next);
        assert_eq!(p.next_page(), None);
        assert_eq!(p.total, Some(40));
    }

    #[test]
    fn stale_answers_and_errors() {
        let p = Paged::default().loading();
        let stale = p.generation;
        let p = p.loading();
        assert_eq!(p.finish(stale, 1, page(&["a"], false)), p);
        let g = p.generation;
        let err = UiError::new(ErrorKind::Offline, "x");
        let failed = p.finish(g, 1, Err(err.clone()));
        assert_eq!(failed.phase, Phase::Failed(err.clone()));
        let ok = p.finish(g, 1, page(&["a"], true)).with_more_loading();
        let more = ok.finish(g, 2, Err(err.clone()));
        assert_eq!(more.more_error, Some(err));
        assert_eq!(more.items.len(), 1);
        assert!(!more.more_loading);
        assert_eq!(Paged::default().idle().phase, Phase::Idle);
        assert!(list_page(vec![m("a")], 2).has_next);
        assert!(!list_page(Vec::new(), 2).has_next);
    }
}
