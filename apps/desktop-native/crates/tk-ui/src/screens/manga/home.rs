//! Manga home `/manga` (worker **l**; checklist `manga.route`, `manga.home`, `manga.home.followed-updates`,
//! `manga.search`, `manga.adult-gate`, `manga.readlist`; classic `MangaHomePage.tsx`, `MangaGenreBrowsePage.tsx`,
//! `FollowedMangaUpdates.tsx`). Three tabs (`data-manga-tab`):
//! - **Discover**: hero (first row of `Backend::manga_home`), "Continue reading" (`ContinueReadVm`, opens the
//!   reader where it stopped), the feed rows (`SectionVm::title_key` = `manga.feed.<id>`, `data-row="<id>"`, incl.
//!   `updates` = new chapters of the titles being read), genre chips (open Search with the genre) and the 18+ zone
//!   (`data-adult-zone`), loaded only after a confirmation unless the mature-content setting shows 18+ titles.
//! - **Search**: query + filters (origin JP / KR / CN / TW, status, format, genres, year, sort), paginated results
//!   (`data-results`, cards `data-kind="manga"`).
//! - **My list**: `ReadListPanel` (read list by status).
//!
//! Search's "Manga" type hands its query over with `screens::manga::open_search`.

use std::time::Duration;

use dioxus::prelude::*;

use super::{MangaCardItem, ReadListPanel, adult_mode, allow_adult_always, reader_route, take_pending_query};
use crate::app::{Route, Services, Ui};
use crate::backend::{ContinueReadVm, MediaVm, ReadHomeVm, SearchQueryVm, SearchSortVm, SectionVm};
use crate::components::{
    Button, ButtonVariant, Chip, ConfirmDialog, EmptyState, ErrorState, HeroCarousel, MediaCard, SegOption,
    Segmented, SkeletonGrid, SkeletonRow, TextField,
};
use crate::i18n::{t, tf};
use crate::model::home::MatureContentVm;
use crate::model::manga::{ADULT_ONLY_TAG, MANGA_FORMATS, MANGA_ORIGINS, MANGA_STATUSES, chapter_label, is_adult_card, read_fraction};
use crate::model::ReadKindVm;
use crate::screens::browse::{Paged, PagedFooter, Phase};
use crate::screens::search::{GENRES, genre_label};

/// Hero slides.
const HERO_ITEMS: usize = 5;
/// Typing pause before the search runs.
const DEBOUNCE_MS: u64 = 350;
/// Years offered by the year filter (from the current one back).
const YEARS: i32 = 40;

/// Tabs of the manga home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Discover,
    Search,
    List,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Discover, Tab::Search, Tab::List];

    fn id(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Search => "search",
            Self::List => "list",
        }
    }
}

/// Filters of the manga search (`SearchQueryVm` with the manga conventions of `live/manga.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MangaFilters {
    /// One of `MANGA_ORIGINS`.
    pub origin: Option<String>,
    /// One of `MANGA_STATUSES`.
    pub status: Option<String>,
    /// One of `MANGA_FORMATS`.
    pub format: Option<String>,
    pub genres: Vec<String>,
    pub year: Option<i32>,
    pub sort: SearchSortVm,
}

impl MangaFilters {
    /// The backend query for `query` + these filters, page `page`.
    pub fn to_query(&self, query: &str, page: u32) -> SearchQueryVm {
        SearchQueryVm {
            query: query.trim().to_owned(),
            genres: self.genres.clone(),
            season: self.origin.clone(),
            status: self.status.clone(),
            format: self.format.clone(),
            year: self.year,
            sort: self.sort,
            page: page.max(1),
            ..SearchQueryVm::default()
        }
    }

    /// Anything to search for (a bare page with no query and no filter shows the hint).
    pub fn active(&self, query: &str) -> bool {
        !query.trim().is_empty()
            || self.origin.is_some()
            || self.status.is_some()
            || self.format.is_some()
            || !self.genres.is_empty()
            || self.year.is_some()
            || self.sort != SearchSortVm::Relevance
    }

    /// `genre` toggled.
    pub fn toggled_genre(&self, genre: &str) -> Self {
        let mut n = self.clone();
        if let Some(i) = n.genres.iter().position(|g| g == genre) {
            n.genres.remove(i);
        } else {
            n.genres.push(genre.to_owned());
        }
        n
    }
}

/// Feed id of a row key (`manga.feed.trending` -> `trending`).
fn feed_id(title_key: &str) -> &str {
    title_key.strip_prefix("manga.feed.").unwrap_or(title_key)
}

fn current_year() -> i32 {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

#[component]
pub fn MangaHomePage() -> Element {
    let pending = use_hook(take_pending_query);
    let mut tab = use_signal(|| if pending.is_some() { Tab::Search } else { Tab::Discover });
    let mut text = use_signal(|| pending.clone().unwrap_or_default());
    let mut query = use_signal(|| pending.clone().unwrap_or_default());
    let mut filters = use_signal(MangaFilters::default);
    let mut typed = use_signal(|| 0_u64);

    let on_input = move |v: String| {
        text.set(v.clone());
        let gen_now = *typed.peek() + 1;
        typed.set(gen_now);
        if !v.trim().is_empty() && *tab.peek() != Tab::Search {
            tab.set(Tab::Search);
        }
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(DEBOUNCE_MS)).await;
            if *typed.peek() == gen_now {
                query.set(v);
            }
        });
    };
    let options: Vec<SegOption> = Tab::ALL.iter().map(|x| (x.id().to_owned(), t(&format!("manga.tab.{}", x.id())))).collect();

    let body = match tab() {
        Tab::Discover => rsx! {
            Discover {
                on_genre: move |g: String| {
                    filters.set(MangaFilters { genres: vec![g], ..MangaFilters::default() });
                    tab.set(Tab::Search);
                },
            }
        },
        Tab::Search => rsx! {
            SearchPanel { query: query(), filters }
        },
        Tab::List => rsx! {
            ReadListPanel { kind: ReadKindVm::Manga }
        },
    };

    rsx! {
        div { class: "tk-page tk-manga", "data-page": "manga", "data-manga-tab": "{tab().id()}",
            div { class: "tk-page__head tk-manga__head",
                h1 { class: "tk-page__title", {t("manga.title")} }
                div { class: "tk-manga__search",
                    TextField {
                        label: t("manga.search.label"),
                        value: pending.clone().unwrap_or_default(),
                        placeholder: t("manga.search.placeholder"),
                        icon: "search".to_string(),
                        oninput: on_input,
                        onsubmit: move |_| {
                            query.set(text.peek().clone());
                            tab.set(Tab::Search);
                        },
                    }
                }
                Segmented {
                    options,
                    selected: tab().id().to_owned(),
                    label: t("manga.title"),
                    on_select: move |id: String| {
                        if let Some(x) = Tab::ALL.into_iter().find(|x| x.id() == id) {
                            tab.set(x);
                        }
                    },
                }
            }
            {body}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Discover
// ---------------------------------------------------------------------------------------------

#[component]
fn Discover(on_genre: EventHandler<String>) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut home = use_resource(move || {
        let backend = backend.clone();
        async move { backend.manga_home().await }
    });
    let body = match &*home.read() {
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
            DiscoverContent { vm: vm.clone(), on_retry: move |_| home.restart(), on_genre }
        },
    };
    rsx! {
        {body}
    }
}

#[component]
fn DiscoverContent(vm: ReadHomeVm, on_retry: EventHandler<()>, on_genre: EventHandler<String>) -> Element {
    let navigator = use_navigator();
    let ui = use_context::<Ui>();
    let reduce = ui.settings.read().reduce_motion;
    let hero: Vec<MediaVm> = vm
        .sections
        .first()
        .and_then(|s| s.items.as_ref().ok())
        .map(|items| items.iter().filter(|m| !is_adult_card(m)).take(HERO_ITEMS).cloned().collect())
        .unwrap_or_default();
    let hero_key = hero.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(",");
    let rows: Vec<SectionVm> = vm.sections.iter().skip(usize::from(!hero.is_empty())).cloned().collect();
    let nothing = hero.is_empty() && vm.continue_reading.is_empty() && rows.iter().all(|s| matches!(&s.items, Ok(i) if i.is_empty()));
    if nothing {
        return rsx! {
            EmptyState {
                icon: "grid".to_string(),
                title: t("manga.home.empty.title"),
                body: t("manga.home.empty.body"),
                action_label: t("common.retry"),
                on_action: move |_| on_retry.call(()),
            }
        };
    }
    rsx! {
        if !hero.is_empty() {
            section { class: "tk-section", "data-hero": "manga",
                HeroCarousel {
                    key: "{hero_key}",
                    items: hero.clone(),
                    kicker: t("manga.feed.trending"),
                    cta: t("manga.read"),
                    autoplay: !reduce,
                    on_open: move |id: String| {
                        navigator.push(Route::MangaDetailsPage { id });
                    },
                }
            }
        }
        if !vm.continue_reading.is_empty() {
            section { class: "tk-section", "data-row": "continue",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title", {t("manga.home.continue")} }
                }
                div { class: "tk-row",
                    for c in vm.continue_reading.clone() {
                        ContinueReadCard { key: "{c.media.id}", item: c.clone() }
                    }
                }
            }
        }
        for s in rows {
            FeedRow { key: "{s.title_key}", section: s.clone(), on_retry }
        }
        section { class: "tk-section", "data-genres": "{GENRES.len()}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("manga.home.genres")} }
            }
            div { class: "tk-chips tk-manga__genres",
                for g in GENRES {
                    Chip { key: "{g}", label: genre_label(g), onclick: move |_| on_genre.call(g.to_owned()) }
                }
            }
        }
        AdultZone {}
    }
}

/// "Continue reading" card: opens the reader on the chapter / page where it stopped.
#[component]
fn ContinueReadCard(item: ContinueReadVm) -> Element {
    let navigator = use_navigator();
    let p = &item.progress;
    let sub = if p.completed {
        tf("manga.home.read_chapter", &[("n", &chapter_label(p.chapter_number))])
    } else {
        tf("manga.home.at_page", &[("n", &chapter_label(p.chapter_number)), ("page", &(p.position + 1).to_string())])
    };
    let (id, chapter) = (item.media.id.clone(), p.chapter.clone());
    rsx! {
        div { class: "tk-manga-card", "data-kind": "manga", "data-continue": "{item.media.id}", "data-page": "{p.position + 1}",
            MediaCard {
                media: item.media.clone(),
                progress: read_fraction(p),
                sub,
                onclick: move |_| {
                    navigator.push(reader_route(&id, &chapter));
                },
            }
        }
    }
}

#[component]
fn FeedRow(section: SectionVm, on_retry: EventHandler<()>) -> Element {
    let id = feed_id(&section.title_key).to_owned();
    let body = match &section.items {
        Err(e) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| on_retry.call(()) }
        },
        Ok(list) if list.is_empty() => rsx! {
            p { class: "tk-section__empty tk-muted", {t("manga.home.row_empty")} }
        },
        Ok(list) => rsx! {
            div { class: "tk-row",
                for m in list.clone() {
                    MangaCardItem { key: "{m.id}", media: m.clone(), sub: if id == "updates" { m.subtitle.clone() } else { String::new() } }
                }
            }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "{id}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t(&section.title_key)} }
                if id == "updates" {
                    span { class: "tk-muted tk-manga__hint", {t("manga.feed.updates_hint")} }
                }
            }
            {body}
        }
    }
}

/// 18+ zone (`manga.adult-gate`): hidden behind a confirmation unless the mature-content setting shows 18+ titles;
/// cards are blurred with `blur`.
#[component]
fn AdultZone() -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let mode = adult_mode(&ui.settings.read());
    let mut open = use_signal(|| false);
    let mut asking = use_signal(|| false);
    let shown = open() || mode == MatureContentVm::Show;
    let backend = services.backend.clone();
    let mut items = use_resource(move || {
        let backend = backend.clone();
        let on = open() || adult_mode(&ui.settings.read()) == MatureContentVm::Show;
        async move {
            if !on {
                return Ok(None);
            }
            let q = SearchQueryVm { tags: vec![ADULT_ONLY_TAG.to_owned()], sort: SearchSortVm::Popularity, adult: true, page: 1, ..SearchQueryVm::default() };
            backend.manga_search(&q).await.map(Some)
        }
    });
    let body = if !shown {
        rsx! {
            div { class: "tk-manga__adult-gate", "data-adult-gate": "locked",
                p { class: "tk-muted", {t("manga.adult.zone_body")} }
                Button {
                    label: t("manga.adult.show_zone"),
                    variant: ButtonVariant::Outlined,
                    icon: "warning".to_string(),
                    onclick: move |_| {
                        if mode == MatureContentVm::Warn {
                            asking.set(true);
                        } else {
                            open.set(true);
                        }
                    },
                }
            }
        }
    } else {
        match &*items.read() {
            None | Some(Ok(None)) => rsx! {
                SkeletonRow { count: 6 }
            },
            Some(Err(e)) => rsx! {
                ErrorState { error: e.clone(), compact: true, on_retry: move |_| items.restart() }
            },
            Some(Ok(Some(page))) if page.items.is_empty() => rsx! {
                p { class: "tk-section__empty tk-muted", {t("manga.home.row_empty")} }
            },
            Some(Ok(Some(page))) => rsx! {
                div { class: "tk-row", "data-adult-gate": "open",
                    for m in page.items.clone() {
                        MangaCardItem { key: "{m.id}", media: m.clone() }
                    }
                }
            },
        }
    };
    rsx! {
        section { class: "tk-section tk-manga__adult", "data-adult-zone": "{shown}", "data-mature": "{mode.id()}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("manga.adult.zone")} }
            }
            {body}
            if asking() {
                ConfirmDialog {
                    title: t("manga.adult.title"),
                    body: t("manga.adult.zone_confirm"),
                    confirm_label: t("manga.adult.continue"),
                    icon: "warning".to_string(),
                    danger: false,
                    on_confirm: move |_| {
                        asking.set(false);
                        open.set(true);
                    },
                    on_cancel: move |_| asking.set(false),
                    button {
                        class: "tk-btn tk-btn--text tk-btn--sm",
                        r#type: "button",
                        onclick: {
                            let services = services.clone();
                            move |_| {
                                allow_adult_always(ui, &services);
                                asking.set(false);
                                open.set(true);
                            }
                        },
                        {t("manga.adult.always")}
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------

/// Optional-choice options: `""` = any, then the values with `<key>.<lowercase value>` labels.
fn any_options(values: &[&str], key: &str) -> Vec<SegOption> {
    let mut v = vec![(String::new(), t("manga.search.any"))];
    v.extend(values.iter().map(|x| ((*x).to_owned(), t(&format!("{key}.{}", x.to_ascii_lowercase())))));
    v
}

#[component]
fn SearchPanel(query: String, filters: Signal<MangaFilters>) -> Element {
    let services = use_context::<Services>();
    let mut paged = use_signal(Paged::default);
    let mut filters = filters;

    let load = use_callback({
        let backend = services.backend.clone();
        move |(q, f, page): (String, MangaFilters, u32)| {
            let next = if page <= 1 { paged.peek().loading() } else { paged.peek().with_more_loading() };
            let generation = next.generation;
            paged.set(next);
            let backend = backend.clone();
            spawn(async move {
                let result = backend.manga_search(&f.to_query(&q, page)).await;
                let next = paged.peek().finish(generation, page, result);
                paged.set(next);
            });
        }
    });

    // Re-run on every query / filter change.
    use_effect(use_reactive!(|query| {
        let f = filters();
        if f.active(&query) {
            load.call((query.clone(), f, 1));
        } else {
            let idle = paged.peek().idle();
            paged.set(idle);
        }
    }));

    let f = filters();
    let pg = paged.read().clone();
    let q_more = query.clone();
    let more = move |_: ()| {
        let next = paged.peek().next_page();
        if let Some(n) = next {
            load.call((q_more.clone(), filters.peek().clone(), n));
        }
    };
    let q_retry = query.clone();
    let year_now = current_year();
    let years: Vec<SegOption> = std::iter::once((String::new(), t("manga.search.any")))
        .chain((0..YEARS).map(|i| ((year_now - i).to_string(), (year_now - i).to_string())))
        .collect();
    let sorts: Vec<SegOption> = SearchSortVm::ALL.iter().map(|s| (s.id().to_owned(), t(&format!("manga.sort.{}", s.id())))).collect();

    let results = match pg.phase {
        Phase::Idle => rsx! {
            EmptyState { icon: "search".to_string(), title: t("manga.search.idle.title"), body: t("manga.search.idle.body") }
        },
        Phase::Loading => rsx! {
            SkeletonGrid { count: 18 }
        },
        Phase::Failed(e) => rsx! {
            ErrorState {
                error: e,
                on_retry: move |_| load.call((q_retry.clone(), filters.peek().clone(), 1)),
            }
        },
        Phase::Ready if pg.items.is_empty() => rsx! {
            EmptyState { icon: "search".to_string(), title: t("manga.search.empty.title"), body: t("manga.search.empty.body") }
        },
        Phase::Ready => rsx! {
            if let Some(total) = pg.total {
                p { class: "tk-muted tk-manga__count", {tf("manga.search.count", &[("n", &total.to_string())])} }
            }
            div { class: "tk-grid tk-manga-grid", "data-results": "{pg.items.len()}",
                for m in pg.items.clone() {
                    MangaCardItem { key: "{m.id}", media: m.clone() }
                }
            }
            PagedFooter { page: pg.page, has_next: pg.has_next, loading: pg.more_loading, error: pg.more_error.clone(), on_more: more }
        },
    };

    rsx! {
        section { class: "tk-section tk-manga__filters", "data-filters": "{f.active(&query)}",
            div { class: "tk-manga__filter",
                span { class: "tk-manga__filter-label", {t("manga.search.origin")} }
                Segmented {
                    options: any_options(&MANGA_ORIGINS, "manga.origin"),
                    selected: f.origin.clone().unwrap_or_default(),
                    label: t("manga.search.origin"),
                    small: true,
                    on_select: move |id: String| {
                        let mut n = filters.peek().clone();
                        n.origin = Some(id).filter(|x| !x.is_empty());
                        filters.set(n);
                    },
                }
            }
            div { class: "tk-manga__filter",
                span { class: "tk-manga__filter-label", {t("manga.search.status")} }
                Segmented {
                    options: any_options(&MANGA_STATUSES, "manga.pub"),
                    selected: f.status.clone().unwrap_or_default(),
                    label: t("manga.search.status"),
                    small: true,
                    on_select: move |id: String| {
                        let mut n = filters.peek().clone();
                        n.status = Some(id).filter(|x| !x.is_empty());
                        filters.set(n);
                    },
                }
            }
            div { class: "tk-manga__filter",
                span { class: "tk-manga__filter-label", {t("manga.search.format")} }
                Segmented {
                    options: any_options(&MANGA_FORMATS, "manga.format"),
                    selected: f.format.clone().unwrap_or_default(),
                    label: t("manga.search.format"),
                    small: true,
                    on_select: move |id: String| {
                        let mut n = filters.peek().clone();
                        n.format = Some(id).filter(|x| !x.is_empty());
                        filters.set(n);
                    },
                }
            }
            div { class: "tk-manga__filter",
                span { class: "tk-manga__filter-label", {t("manga.search.sort")} }
                Segmented {
                    options: sorts,
                    selected: f.sort.id().to_owned(),
                    label: t("manga.search.sort"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(s) = SearchSortVm::ALL.into_iter().find(|s| s.id() == id) {
                            let mut n = filters.peek().clone();
                            n.sort = s;
                            filters.set(n);
                        }
                    },
                }
            }
            label { class: "tk-manga__filter",
                span { class: "tk-manga__filter-label", {t("manga.search.year")} }
                select {
                    class: "tk-manga__select",
                    "aria-label": t("manga.search.year"),
                    "data-year": "{f.year.map(|y| y.to_string()).unwrap_or_default()}",
                    onchange: move |e| {
                        let mut n = filters.peek().clone();
                        n.year = e.value().parse::<i32>().ok();
                        filters.set(n);
                    },
                    for (v , label) in years {
                        option { key: "{v}", value: "{v}", selected: f.year.map(|y| y.to_string()).unwrap_or_default() == v, "{label}" }
                    }
                }
            }
            div { class: "tk-chips tk-manga__genres", "aria-label": t("manga.search.genres"),
                for g in GENRES {
                    Chip {
                        key: "{g}",
                        label: genre_label(g),
                        on: f.genres.iter().any(|x| x == g),
                        onclick: move |_| {
                            let n = filters.peek().toggled_genre(g);
                            filters.set(n);
                        },
                    }
                }
            }
            if f != MangaFilters::default() {
                Button {
                    label: t("manga.search.clear"),
                    variant: ButtonVariant::Text,
                    icon: "close".to_string(),
                    onclick: move |_| filters.set(MangaFilters::default()),
                }
            }
        }
        section { class: "tk-section tk-manga__results", {results} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_build_the_manga_query() {
        let f = MangaFilters {
            origin: Some("kr".into()),
            status: Some("releasing".into()),
            format: Some("MANGA".into()),
            genres: vec!["Action".into()],
            year: Some(2021),
            sort: SearchSortVm::Score,
        };
        let q = f.to_query("  solo leveling ", 0);
        assert_eq!(q.query, "solo leveling");
        assert_eq!(q.season.as_deref(), Some("kr"));
        assert_eq!(q.status.as_deref(), Some("releasing"));
        assert_eq!(q.format.as_deref(), Some("MANGA"));
        assert_eq!(q.year, Some(2021));
        assert_eq!(q.page, 1);
        assert!(!q.adult);
        assert!(f.active(""));
        assert!(!MangaFilters::default().active("  "));
        assert!(MangaFilters::default().active("x"));
        let g = MangaFilters::default().toggled_genre("Drama");
        assert_eq!(g.genres, vec!["Drama".to_owned()]);
        assert!(g.toggled_genre("Drama").genres.is_empty());
        assert_eq!(feed_id("manga.feed.updates"), "updates");
        assert!(current_year() >= 2024);
    }
}
