//! Search: debounced query, stale-response guard (generation counter), recent searches (6, local),
//! type/origin chips, filter side sheet, result counter and master-detail on wide windows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::MediaVm;
use crate::components::{
    Banner, Button, ButtonVariant, Chip, EmptyState, ErrorState, IconButton, IconButtonSize, Kbd, MediaCard,
    MediaCover, Pill, Sheet, SkeletonGrid, Tone, use_uncontrolled_input,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{
    LoadState, PaneLayout, SEARCH_DEBOUNCE_MS, SearchAction, SearchState, UiError, format_unix_utc, normalize_query,
    pane_layout, query_is_searchable, reduce_search,
};
use crate::screens::details::DetailsView;
use crate::shortcuts::Shortcut;

// ---------------------------------------------------------------------------------------------
// Recent searches (pure helpers + web view localStorage; the backend has no call for them yet)
// ---------------------------------------------------------------------------------------------

pub const HISTORY_MAX: usize = 6;
const HISTORY_KEY: &str = "tk.search.history";

/// Session copy; filled once from localStorage on the first visit.
static HISTORY: GlobalSignal<Vec<String>> = Signal::global(Vec::new);
static HISTORY_LOADED: AtomicBool = AtomicBool::new(false);
/// The page as it was left (query, results, open item, origin, filters): Search is rebuilt on every visit, so
/// coming back from details or the player would otherwise start empty.
static KEPT: GlobalSignal<Option<Kept>> = Signal::global(|| None);

#[derive(Clone)]
struct Kept {
    state: SearchState<MediaVm>,
    selected: Option<String>,
    origin: Option<String>,
    filters: SearchFilters,
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

// ---------------------------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------------------------

/// AniList genre names (provider data, shown untranslated like `DetailsVm::genres`).
const GENRES: [&str; 18] = [
    "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Horror", "Mahou Shoujo", "Mecha", "Music",
    "Mystery", "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural", "Thriller",
];
/// i18n suffixes: `search.season.<id>`.
const SEASONS: [&str; 4] = ["winter", "spring", "summer", "fall"];
/// i18n suffixes: `status.<id>`.
const STATUSES: [&str; 3] = ["releasing", "finished", "upcoming"];
const YEARS_SHOWN: i32 = 7;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchFilters {
    pub genre: Option<String>,
    pub year: Option<String>,
    /// One of [`SEASONS`].
    pub season: Option<String>,
    /// One of [`STATUSES`].
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterField {
    Genre,
    Year,
    Season,
    Status,
}

impl SearchFilters {
    pub fn is_empty(&self) -> bool {
        self.genre.is_none() && self.year.is_none() && self.season.is_none() && self.status.is_none()
    }

    /// Filters `Backend::search` cannot apply yet (only the year is checked locally, on the subtitle).
    pub fn has_unapplied(&self) -> bool {
        self.genre.is_some() || self.season.is_some() || self.status.is_some()
    }

    pub fn get(&self, f: FilterField) -> Option<&String> {
        match f {
            FilterField::Genre => self.genre.as_ref(),
            FilterField::Year => self.year.as_ref(),
            FilterField::Season => self.season.as_ref(),
            FilterField::Status => self.status.as_ref(),
        }
    }

    fn slot(&mut self, f: FilterField) -> &mut Option<String> {
        match f {
            FilterField::Genre => &mut self.genre,
            FilterField::Year => &mut self.year,
            FilterField::Season => &mut self.season,
            FilterField::Status => &mut self.status,
        }
    }

    /// Select `value`, or clear it when it is already selected (chip toggle).
    pub fn toggled(&self, f: FilterField, value: &str) -> Self {
        let mut n = self.clone();
        let slot = n.slot(f);
        *slot = if slot.as_deref() == Some(value) { None } else { Some(value.to_owned()) };
        n
    }

    pub fn cleared(&self, f: FilterField) -> Self {
        let mut n = self.clone();
        *n.slot(f) = None;
        n
    }

    /// `(field, value)` of every active filter, in sheet order.
    pub fn active(&self) -> Vec<(FilterField, String)> {
        [FilterField::Genre, FilterField::Year, FilterField::Season, FilterField::Status]
            .into_iter()
            .filter_map(|f| self.get(f).map(|v| (f, v.clone())))
            .collect()
    }
}

/// The part of the filters the UI can apply on `MediaVm` alone: the year must appear in the subtitle
/// (`2024 - 12 ep`).
pub fn apply_local_filters(items: &[MediaVm], f: &SearchFilters) -> Vec<MediaVm> {
    items.iter().filter(|m| f.year.as_deref().is_none_or(|y| m.subtitle.contains(y))).cloned().collect()
}

fn filter_label(f: FilterField, value: &str) -> String {
    match f {
        FilterField::Season => t(&format!("search.season.{value}")),
        FilterField::Status => t(&format!("status.{value}")),
        FilterField::Genre | FilterField::Year => value.to_owned(),
    }
}

fn current_year() -> i32 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format_unix_utc(now).get(..4).and_then(|y| y.parse().ok()).unwrap_or(2025)
}

/// `85` -> `8.5`.
fn score_text(score: u32) -> String {
    format!("{:.1}", f64::from(score) / 10.0)
}

/// The selected id if it is still in `items`, else the first result (wide layout selects one).
fn effective_selection(items: &[MediaVm], selected: Option<String>) -> Option<String> {
    selected.filter(|s| items.iter().any(|m| m.id == *s)).or_else(|| items.first().map(|m| m.id.clone()))
}

fn apply(sig: &mut Signal<SearchState<MediaVm>>, action: SearchAction<MediaVm>) {
    let next = reduce_search(&sig.peek(), action);
    sig.set(next);
}

#[component]
pub fn SearchPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let backend = services.backend.clone();

    let kept = KEPT.peek().clone();
    // A search still loading when the page was left has no task to finish it: it runs again below.
    let rerun = kept.as_ref().filter(|k| matches!(k.state.results, Some(LoadState::Loading))).map(|k| k.state.query.clone());
    let mut state = use_signal(|| kept.as_ref().map(|k| k.state.clone()).unwrap_or_default());
    let mut selected = use_signal(|| kept.as_ref().and_then(|k| k.selected.clone()));
    // `None` = metadata providers; `Some(ext id)` = search through that extension.
    let mut origin = use_signal(|| kept.as_ref().and_then(|k| k.origin.clone()));
    let mut filters = use_signal(|| kept.as_ref().map(|k| k.filters.clone()).unwrap_or_default());
    use_drop(move || {
        *KEPT.write() = Some(Kept {
            state: state.peek().clone(),
            selected: selected.peek().clone(),
            origin: origin.peek().clone(),
            filters: filters.peek().clone(),
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

    let run_search = use_callback({
        let backend = backend.clone();
        move |q: String| {
            apply(&mut state, SearchAction::Typed(q.clone()));
            if !query_is_searchable(&q) {
                return;
            }
            let generation = state.peek().generation;
            let backend = backend.clone();
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(SEARCH_DEBOUNCE_MS)).await;
                if state.peek().generation != generation {
                    return;
                }
                apply(&mut state, SearchAction::Started(generation));
                let result = backend.search(&normalize_query(&q), 1).await;
                apply(&mut state, SearchAction::Finished { generation, result });
            });
        }
    });

    use_hook(move || {
        if let Some(q) = rerun {
            run_search.call(q);
        }
    });

    let open_details = use_callback(move |id: String| {
        remember(&state.peek().query);
        navigator.push(Route::DetailsPage { id });
    });

    let dual = pane_layout((ui.width)()) == PaneLayout::Dual;
    let query = state.read().query.clone();
    let (input_generation, record_input) = use_uncontrolled_input(&query);
    let placeholder = t("search.placeholder");
    let hint = t("search.hint");
    let clear_label = t("search.clear");
    let type_label = t("search.type");
    let active_filters = filters.read().clone();
    let m1 = t("search.m1");
    let by_extension = origin.read().is_some();

    // Enabled, loadable extensions become origin chips.
    let ext_chips: Vec<(String, String)> = match &*exts.read() {
        Some(Ok(list)) => list
            .iter()
            .filter(|e| e.enabled && e.compat.can_enable())
            .map(|e| (e.id.clone(), e.name.clone()))
            .collect(),
        _ => Vec::new(),
    };

    // Results after the local filters (`None` = nothing searched / not loaded).
    let ready: Option<Vec<MediaVm>> = match &state.read().results {
        Some(LoadState::Ready(raw)) => Some(apply_local_filters(raw, &active_filters)),
        _ => None,
    };
    let detail_id = if by_extension { None } else { ready.as_deref().and_then(|items| effective_selection(items, selected())) };
    let split = dual && detail_id.is_some();

    let chips = rsx! {
        div { class: "tk-chips tk-search__chips", role: "group", "aria-label": "{type_label}",
            Chip { label: t("type.anime"), on: true }
            span { title: "{m1}", Chip { label: t("type.manga"), disabled: true } }
            span { title: "{m1}", Chip { label: t("type.novel"), disabled: true } }
            span { class: "tk-search__sep", "aria-hidden": "true" }
            Chip {
                label: t("source.metadata"),
                icon: "sync".to_string(),
                on: !by_extension,
                onclick: move |_| origin.set(None),
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
                            onclick: move |_| origin.set(Some(pick.clone())),
                        }
                    }
                }
            }
            for (field , value) in active_filters.active() {
                Chip {
                    key: "f-{value}",
                    label: filter_label(field, &value),
                    on: true,
                    dismiss_label: t("common.remove"),
                    on_dismiss: move |_| {
                        let next = filters.peek().cleared(field);
                        filters.set(next);
                    },
                }
            }
        }
    };

    let results = if by_extension {
        // `Backend::search` only queries the metadata providers (wave 1 contract).
        rsx! {
            ErrorState { error: UiError::not_wired("search by extension"), retry: false }
        }
    } else {
        match &state.read().results {
            None => {
                let history = HISTORY.read().clone();
                rsx! {
                    EmptyState { icon: "search".to_string(), title: t("search.empty.title"), body: t("search.empty.body") }
                    if !history.is_empty() {
                        section { class: "tk-section",
                            h2 { class: "tk-section__title tk-section__title--sm tk-muted", {t("search.history")} }
                            div { class: "tk-search__history",
                                for q in history {
                                    {
                                        let (again, gone) = (q.clone(), q.clone());
                                        rsx! {
                                            div { key: "{q}", class: "tk-search__hist",
                                                button {
                                                    class: "tk-search__histq",
                                                    r#type: "button",
                                                    onclick: move |_| run_search.call(again.clone()),
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
                }
            }
            Some(LoadState::Loading) => rsx! { SkeletonGrid { count: 12 } },
            Some(LoadState::Failed(e)) => {
                let q = state.peek().query.clone();
                rsx! {
                    ErrorState { error: e.clone(), on_retry: move |_| run_search.call(q.clone()) }
                }
            }
            Some(LoadState::Ready(_)) => {
                let items = ready.clone().unwrap_or_default();
                if items.is_empty() {
                    let has_filters = !active_filters.is_empty();
                    rsx! {
                        EmptyState {
                            icon: "search".to_string(),
                            title: t("search.no_results.title"),
                            body: tf("search.no_results.body", &[("query", &query)]),
                            action_label: if has_filters { t("search.reset") } else { String::new() },
                            on_action: move |_| filters.set(SearchFilters::default()),
                        }
                    }
                } else {
                    let count = tn("search.results", items.len());
                    let sel = detail_id.clone();
                    rsx! {
                        p { class: "tk-search__count tk-muted", role: "status", "{count}" }
                        if active_filters.has_unapplied() {
                            Banner { text: t("error.not_wired"), icon: "info".to_string(), tone: Tone::Warn }
                        }
                        if dual {
                            div { class: "tk-list tk-search__results", role: "listbox", "aria-label": "{count}",
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
                                                on_open: move |_| open_details.call(open_id.clone()),
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "tk-grid",
                                for m in items {
                                    {
                                        let id = m.id.clone();
                                        rsx! {
                                            MediaCard { key: "{m.id}", media: m.clone(), onclick: move |_| open_details.call(id.clone()) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        div { class: "tk-page tk-page--search", "data-dual": "{split}",
            div { class: "tk-search__list",
                div { class: "tk-page__head",
                    h1 { class: "tk-page__title", {t("nav.search")} }
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
                                initial_value: "{query}",
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
                                    run_search.call(e.value())
                                },
                                onkeydown: move |e| {
                                    if e.key() == Key::Enter {
                                        remember(&state.peek().query);
                                    }
                                },
                            }
                        }
                    })}
                    Kbd { keys: "Ctrl K".to_string() }
                    if !query.is_empty() {
                        IconButton {
                            icon: "close".to_string(),
                            label: clear_label.clone(),
                            onclick: move |_| {
                                apply(&mut state, SearchAction::Typed(String::new()));
                                selected.set(None);
                            },
                        }
                    }
                    IconButton {
                        icon: "tune".to_string(),
                        label: t("search.filters"),
                        toggled: !active_filters.is_empty(),
                        onclick: move |_| filters_open.set(true),
                    }
                }
                {chips}
                {results}
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
                        filters.set(f);
                        filters_open.set(false);
                    },
                    on_close: move |_| filters_open.set(false),
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

/// Side sheet with genre / year / season / status chips. Edits a draft; Apply commits it, Reset
/// commits the empty set.
#[component]
fn FilterSheet(value: SearchFilters, on_apply: EventHandler<SearchFilters>, on_close: EventHandler<()>) -> Element {
    let mut draft = use_signal(|| value.clone());
    let year = current_year();
    let years: Vec<String> = (0..YEARS_SHOWN).map(|i| (year - i).to_string()).collect();
    let groups: Vec<(FilterField, String, Vec<String>)> = vec![
        (FilterField::Genre, t("search.genre"), GENRES.iter().map(|g| (*g).to_owned()).collect()),
        (FilterField::Year, t("search.year"), years),
        (FilterField::Season, t("search.season"), SEASONS.iter().map(|s| (*s).to_owned()).collect()),
        (FilterField::Status, t("search.status"), STATUSES.iter().map(|s| (*s).to_owned()).collect()),
    ];
    let current = draft.read().clone();
    rsx! {
        Sheet {
            title: t("search.filters"),
            on_close: move |_| on_close.call(()),
            footer: rsx! {
                Button {
                    label: t("search.reset"),
                    variant: ButtonVariant::Text,
                    onclick: move |_| on_apply.call(SearchFilters::default()),
                }
                Button { label: t("search.apply"), onclick: move |_| on_apply.call(draft.peek().clone()) }
            },
            div { class: "tk-filters",
                for (field , label , values) in groups {
                    div { key: "{label}", class: "tk-filters__group", role: "group", "aria-label": "{label}",
                        h3 { class: "tk-filters__label", "{label}" }
                        div { class: "tk-chips",
                            for v in values {
                                {
                                    let on = current.get(field).map(String::as_str) == Some(v.as_str());
                                    let text = filter_label(field, &v);
                                    let pick = v.clone();
                                    rsx! {
                                        Chip {
                                            key: "{v}",
                                            label: text,
                                            on,
                                            onclick: move |_| {
                                                let next = draft.peek().toggled(field, &pick);
                                                draft.set(next);
                                            },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: &str, sub: &str) -> MediaVm {
        MediaVm { id: id.into(), title: id.into(), cover: None, color: None, subtitle: sub.into(), score: None }
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
    fn filters_toggle_and_year_applies_locally() {
        let f = SearchFilters::default().toggled(FilterField::Year, "2024");
        assert_eq!(f.year.as_deref(), Some("2024"));
        assert!(f.toggled(FilterField::Year, "2024").is_empty());
        assert!(!f.has_unapplied());
        let items = [m("a", "2024 - 12 ep"), m("b", "2019 - 24 ep")];
        assert_eq!(apply_local_filters(&items, &f).len(), 1);
        assert_eq!(apply_local_filters(&items, &SearchFilters::default()).len(), 2);
        let g = f.toggled(FilterField::Genre, "Action");
        assert!(g.has_unapplied());
        assert_eq!(g.active().len(), 2);
        assert!(g.cleared(FilterField::Genre).cleared(FilterField::Year).is_empty());
        assert_eq!(score_text(85), "8.5");
    }

    #[test]
    fn selection_falls_back_to_the_first_result() {
        let items = [m("a", ""), m("b", "")];
        assert_eq!(effective_selection(&items, Some("b".into())).as_deref(), Some("b"));
        assert_eq!(effective_selection(&items, Some("zz".into())).as_deref(), Some("a"));
        assert_eq!(effective_selection(&[], None), None);
    }
}
