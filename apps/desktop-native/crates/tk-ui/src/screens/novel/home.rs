//! Novels home `/novels` (worker **m**; checklist `novel.route`, `novel.home`, `novel.search`; classic
//! `NovelHomePage.tsx`, `NovelGenreBrowsePage.tsx`): hero + rows from `Backend::novel_home()`
//! (`SectionVm::title_key` = `novel.feed.<id>`), "Continue reading" (opens the reader where it was left), genre chips
//! and a filtered search (`novel_search`: text, genres, status, origin, order) shown as a paged grid instead of the
//! rows while any filter is set. The Search screen's "Novel" type hands its query over with
//! [`set_pending_query`] before navigating here (the route carries no query string).

use std::sync::Mutex;

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{
    ContinueReadVm, MediaVm, ReadHomeVm, SearchPageVm, SearchQueryVm, SearchSortVm, SectionVm,
};
use crate::components::{
    Button, ButtonSize, ButtonVariant, Chip, EmptyState, ErrorState, HeroCarousel, MediaCard, MediaCover,
    SkeletonGrid, SkeletonRow,
};
use crate::i18n::{t, tf};
use crate::model::UiError;
use crate::model::novel::{chapter_label, chapter_number_of};

/// AniList genres offered as chips (same list as `tk_core::novel::anilist::GENRES`).
pub const GENRES: [&str; 16] = [
    "Action",
    "Adventure",
    "Comedy",
    "Drama",
    "Ecchi",
    "Fantasy",
    "Horror",
    "Mahou Shoujo",
    "Mecha",
    "Mystery",
    "Psychological",
    "Romance",
    "Sci-Fi",
    "Slice of Life",
    "Sports",
    "Supernatural",
];
/// Status filter ids (`search.status.<id>` texts of worker a's namespace).
pub const STATUSES: [&str; 5] = ["releasing", "finished", "upcoming", "cancelled", "hiatus"];
/// Country of origin filter (`novel.origin.<id>`), sent as an `origin:XX` tag.
pub const ORIGINS: [&str; 4] = ["JP", "KR", "CN", "TW"];
const HERO_ITEMS: usize = 5;

static PENDING_QUERY: Mutex<Option<String>> = Mutex::new(None);

/// Search text the Novels home picks up on its next mount (the Search screen's "Novel" type).
pub fn set_pending_query(query: &str) {
    if let Ok(mut g) = PENDING_QUERY.lock() {
        *g = Some(query.trim().to_owned()).filter(|q| !q.is_empty());
    }
}

fn take_pending_query() -> Option<String> {
    PENDING_QUERY.lock().ok().and_then(|mut g| g.take())
}

/// The novel search filters of this screen.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct NovelFilters {
    pub query: String,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub origin: Option<String>,
    pub sort: SearchSortVm,
}

impl NovelFilters {
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty()
            || !self.genres.is_empty()
            || self.status.is_some()
            || self.origin.is_some()
    }

    /// The backend query of page `page`.
    pub fn to_query(&self, page: u32) -> SearchQueryVm {
        SearchQueryVm {
            query: self.query.trim().to_owned(),
            genres: self.genres.clone(),
            tags: self.origin.iter().map(|o| format!("origin:{o}")).collect(),
            status: self.status.clone(),
            sort: self.sort,
            page: page.max(1),
            ..SearchQueryVm::default()
        }
    }

    pub fn toggle_genre(&mut self, g: &str) {
        if let Some(i) = self.genres.iter().position(|x| x == g) {
            self.genres.remove(i);
        } else {
            self.genres.push(g.to_owned());
        }
    }
}

/// Reader route of a "Continue reading" card.
pub fn resume_route(c: &ContinueReadVm) -> Route {
    // From the AniList list only (no reader position here): open the title.
    if c.progress.chapter.ext_id.is_empty() {
        return Route::NovelDetailsPage { id: c.media.id.clone() };
    }
    Route::NovelReaderPage {
        id: c.media.id.clone(),
        ext_id: c.progress.chapter.ext_id.clone(),
        chapter_id: c.progress.chapter.chapter_id.clone(),
    }
}

#[component]
pub fn NovelHomePage() -> Element {
    let mut filters = use_signal(|| NovelFilters {
        query: take_pending_query().unwrap_or_default(),
        ..NovelFilters::default()
    });
    let f = filters();
    let active = f.is_active();
    let query_text = f.query.clone();
    rsx! {
        div { class: "tk-page tk-novel", "data-page": "novels", "data-searching": "{active}",
            div { class: "tk-page__head tk-novel__head",
                h1 { class: "tk-page__title", {t("novel.title")} }
                p { class: "tk-page__intro tk-muted", {t("novel.home.intro")} }
            }
            div { class: "tk-novel__search", role: "search",
                input {
                    class: "tk-novel__query",
                    r#type: "search",
                    "aria-label": t("novel.search.label"),
                    placeholder: t("novel.search.placeholder"),
                    value: "{query_text}",
                    onchange: move |e| {
                        let v = e.value();
                        filters.with_mut(|f| f.query = v);
                    },
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            filters.with_mut(|f| f.query.clear());
                        }
                    },
                }
                FilterBar { filters }
            }
            div { class: "tk-novel__genres", role: "group", "aria-label": t("novel.genres"),
                for g in GENRES {
                    Chip {
                        key: "{g}",
                        label: crate::screens::search::genre_label(g),
                        on: f.genres.iter().any(|x| x == g),
                        onclick: move |_| filters.with_mut(|f| f.toggle_genre(g)),
                    }
                }
            }
            if active {
                // Keyed by the filters: new filters remount the grid at page 1.
                {std::iter::once(f.clone()).map(|f| {
                    let k = format!("{f:?}");
                    rsx! { SearchGrid { key: "{k}", filters: f } }
                })}
            } else {
                HomeRows {}
            }
        }
    }
}

#[component]
fn FilterBar(filters: Signal<NovelFilters>) -> Element {
    let mut filters = filters;
    let f = filters();
    let status = f.status.clone().unwrap_or_default();
    let origin = f.origin.clone().unwrap_or_default();
    let sort = f.sort.id();
    rsx! {
        div { class: "tk-novel__filters",
            label { class: "tk-novel__filter",
                span { class: "tk-novel__filter-label", {t("novel.filter.status")} }
                select {
                    "data-filter": "status",
                    value: "{status}",
                    onchange: move |e| {
                        let v = e.value();
                        filters.with_mut(|f| f.status = Some(v).filter(|s| !s.is_empty()));
                    },
                    option { value: "", selected: status.is_empty(), {t("novel.filter.any")} }
                    for s in STATUSES {
                        option { key: "{s}", value: "{s}", selected: status == s, {t(&format!("search.status.{s}"))} }
                    }
                }
            }
            label { class: "tk-novel__filter",
                span { class: "tk-novel__filter-label", {t("novel.filter.origin")} }
                select {
                    "data-filter": "origin",
                    value: "{origin}",
                    onchange: move |e| {
                        let v = e.value();
                        filters.with_mut(|f| f.origin = Some(v).filter(|s| !s.is_empty()));
                    },
                    option { value: "", selected: origin.is_empty(), {t("novel.filter.any")} }
                    for o in ORIGINS {
                        option { key: "{o}", value: "{o}", selected: origin == o, {t(&format!("novel.origin.{o}"))} }
                    }
                }
            }
            label { class: "tk-novel__filter",
                span { class: "tk-novel__filter-label", {t("novel.filter.sort")} }
                select {
                    "data-filter": "sort",
                    value: "{sort}",
                    onchange: move |e| {
                        let v = e.value();
                        let s = SearchSortVm::ALL.into_iter().find(|x| x.id() == v).unwrap_or_default();
                        filters.with_mut(|f| f.sort = s);
                    },
                    for s in SearchSortVm::ALL {
                        option { key: "{s.id()}", value: "{s.id()}", selected: sort == s.id(), {t(&format!("search.sort.{}", s.id()))} }
                    }
                }
            }
            if f.is_active() {
                Button {
                    label: t("novel.filter.clear"),
                    icon: "close".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: move |_| filters.set(NovelFilters::default()),
                }
            }
        }
    }
}

/// Paged results of the filtered search ("Load more").
#[component]
fn SearchGrid(filters: NovelFilters) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut items = use_signal(Vec::<MediaVm>::new);
    let mut page = use_signal(|| 0_u32);
    let mut has_next = use_signal(|| true);
    let mut total = use_signal(|| None::<u32>);
    let mut loading = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let load = use_callback(move |_: ()| {
        if *loading.peek() || !*has_next.peek() {
            return;
        }
        let backend = services.backend.clone();
        let next = *page.peek() + 1;
        let q = filters.to_query(next);
        loading.set(true);
        error.set(None);
        spawn(async move {
            let res: Result<SearchPageVm, UiError> = backend.novel_search(&q).await;
            loading.set(false);
            match res {
                Ok(p) => {
                    page.set(p.page.max(next));
                    has_next.set(p.has_next);
                    total.set(p.total);
                    items.with_mut(|v| {
                        for m in p.items {
                            if !v.iter().any(|x| x.id == m.id) {
                                v.push(m);
                            }
                        }
                    });
                }
                Err(e) => error.set(Some(e)),
            }
        });
    });
    use_effect(move || load.call(()));

    let list = items();
    let count = total().map_or_else(|| list.len().to_string(), |n| n.to_string());
    rsx! {
        section { class: "tk-section tk-novel__results", "data-results": "{list.len()}", "aria-busy": "{loading()}",
            if !list.is_empty() {
                p { class: "tk-novel__count tk-muted", role: "status", {tf("novel.search.count", &[("n", &count)])} }
                div { class: "tk-grid",
                    for m in list.clone() {
                        {
                            let id = m.id.clone();
                            rsx! {
                                div { key: "{m.id}", "data-kind": "novel",
                                    MediaCard {
                                        media: m.clone(),
                                        onclick: move |_| {
                                            navigator.push(Route::NovelDetailsPage { id: id.clone() });
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if loading() && list.is_empty() {
                SkeletonGrid { count: 12 }
            } else if let Some(e) = error() {
                ErrorState { error: e, compact: !list.is_empty(), on_retry: move |_| load.call(()) }
            } else if list.is_empty() {
                EmptyState { icon: "search".to_string(), title: t("novel.search.empty.title"), body: t("novel.search.empty.body") }
            } else if has_next() {
                div { class: "tk-novel__more",
                    Button {
                        label: t("novel.search.more"),
                        variant: ButtonVariant::Tonal,
                        disabled: loading(),
                        onclick: move |_| load.call(()),
                    }
                }
            }
        }
    }
}

#[component]
fn HomeRows() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut home = use_resource(move || {
        let backend = backend.clone();
        async move { backend.novel_home().await }
    });
    match &*home.read() {
        None => rsx! {
            section { class: "tk-section", "aria-busy": "true",
                div { class: "tk-skel tk-skel--hero", "aria-hidden": "true" }
            }
            section { class: "tk-section", "aria-busy": "true", SkeletonRow { count: 8 } }
            section { class: "tk-section", "aria-busy": "true", SkeletonRow { count: 8 } }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| home.restart() }
        },
        Some(Ok(vm)) => rsx! {
            HomeContent { vm: vm.clone(), on_retry: move |_| home.restart() }
        },
    }
}

#[component]
fn HomeContent(vm: ReadHomeVm, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    // The trending row feeds the hero (it is listed again below only when the hero is empty).
    let trending = vm.sections.iter().find(|s| s.title_key == "novel.feed.trending");
    let hero: Vec<MediaVm> = trending
        .and_then(|s| s.items.as_ref().ok())
        .map(|items| items.iter().take(HERO_ITEMS).cloned().collect())
        .unwrap_or_default();
    let hero_key = hero.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(",");
    let rows: Vec<SectionVm> = vm
        .sections
        .iter()
        .filter(|s| hero.is_empty() || s.title_key != "novel.feed.trending")
        .cloned()
        .collect();
    let has_rows = rows
        .iter()
        .any(|s| !matches!(&s.items, Ok(items) if items.is_empty()));
    if hero.is_empty() && vm.continue_reading.is_empty() && !has_rows {
        return rsx! {
            EmptyState {
                icon: "bookmark".to_string(),
                title: t("novel.home.empty.title"),
                body: t("novel.home.empty.body"),
                action_label: t("common.retry"),
                on_action: move |_| on_retry.call(()),
            }
        };
    }
    rsx! {
        if !hero.is_empty() {
            section { class: "tk-section", "data-feed": "hero",
                HeroCarousel {
                    key: "{hero_key}",
                    items: hero.clone(),
                    kicker: t("novel.feed.trending"),
                    cta: t("novel.read"),
                    on_open: move |id: String| {
                        navigator.push(Route::NovelDetailsPage { id });
                    },
                }
            }
        }
        if !vm.continue_reading.is_empty() {
            section { class: "tk-section", "data-feed": "continue",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title", {t("novel.home.continue")} }
                }
                div { class: "tk-row",
                    for c in vm.continue_reading.clone() {
                        ContinueReadCard { key: "{c.media.id}", item: c.clone() }
                    }
                }
            }
        }
        for s in rows {
            FeedRow { key: "{s.title_key}", section: s.clone() }
        }
    }
}

/// "Continue reading" tile: poster, title, chapter and paragraph progress; opens the reader there.
#[component]
fn ContinueReadCard(item: ContinueReadVm) -> Element {
    let navigator = use_navigator();
    let p = &item.progress;
    let number = if p.chapter_number > 0.0 {
        Some(p.chapter_number)
    } else {
        chapter_number_of(&p.chapter.chapter_id)
    };
    let sub = match number {
        Some(n) => tf("novel.chapter.n", &[("n", &chapter_label(n))]),
        None => t("novel.home.resume"),
    };
    let fraction = p
        .total
        .filter(|t| *t > 0)
        .map(|t| f64::from(p.position) / f64::from(t));
    let route = resume_route(&item);
    rsx! {
        button {
            class: "tk-card tk-novel__continue",
            r#type: "button",
            "aria-label": "{item.media.title}",
            "data-paragraph": "{p.position}",
            onclick: move |_| {
                navigator.push(route.clone());
            },
            MediaCover {
                src: item.media.cover.clone(),
                color: item.media.color.clone(),
                title: item.media.title.clone(),
                progress: fraction,
            }
            div { class: "tk-card__title", "{item.media.title}" }
            div { class: "tk-card__sub", "{sub}" }
        }
    }
}

#[component]
fn FeedRow(section: SectionVm) -> Element {
    let navigator = use_navigator();
    let feed = section
        .title_key
        .strip_prefix("novel.feed.")
        .unwrap_or(&section.title_key)
        .to_owned();
    let title = t(&section.title_key);
    rsx! {
        section { class: "tk-section", "data-feed": "{feed}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", "{title}" }
            }
            match section.items.clone() {
                Ok(list) if list.is_empty() => rsx! {
                    p { class: "tk-section__empty tk-muted", {t("novel.home.row_empty")} }
                },
                Ok(list) => rsx! {
                    div { class: "tk-row",
                        for m in list {
                            {
                                let id = m.id.clone();
                                rsx! {
                                    MediaCard {
                                        key: "{m.id}",
                                        media: m.clone(),
                                        onclick: move |_| {
                                            navigator.push(Route::NovelDetailsPage { id: id.clone() });
                                        },
                                    }
                                }
                            }
                        }
                    }
                },
                Err(e) => rsx! {
                    ErrorState { error: e, compact: true, retry: false }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ChapterRefVm, ReadKindVm, ReadProgressVm};

    #[test]
    fn filters_build_the_query() {
        let mut f = NovelFilters::default();
        assert!(!f.is_active());
        f.toggle_genre("Fantasy");
        f.origin = Some("KR".into());
        let q = f.to_query(0);
        assert_eq!(q.genres, vec!["Fantasy".to_owned()]);
        assert_eq!(q.tags, vec!["origin:KR".to_owned()]);
        assert_eq!(q.page, 1);
        f.toggle_genre("Fantasy");
        assert!(f.genres.is_empty());
        assert!(f.is_active());
    }

    #[test]
    fn pending_query_is_taken_once() {
        set_pending_query("  overlord ");
        assert_eq!(take_pending_query().as_deref(), Some("overlord"));
        assert_eq!(take_pending_query(), None);
    }

    #[test]
    fn resume_goes_to_the_saved_chapter() {
        let c = ContinueReadVm {
            media: MediaVm {
                id: "9".into(),
                title: "T".into(),
                cover: None,
                color: None,
                subtitle: String::new(),
                score: None,
            },
            kind: ReadKindVm::Novel,
            progress: ReadProgressVm {
                chapter: ChapterRefVm {
                    ext_id: "toko".into(),
                    chapter_id: "abc".into(),
                },
                chapter_number: 2.0,
                position: 7,
                total: Some(20),
                completed: false,
                updated_unix: 1,
            },
        };
        assert_eq!(
            resume_route(&c),
            Route::NovelReaderPage {
                id: "9".into(),
                ext_id: "toko".into(),
                chapter_id: "abc".into()
            }
        );
    }
}
