//! Search: debounced query, stale-response guard (generation counter), master-detail on wide windows.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::MediaVm;
use crate::components::{EmptyState, ErrorState, MediaCard, SkeletonGrid};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{
    LoadState, PaneLayout, SEARCH_DEBOUNCE_MS, SearchAction, SearchState, normalize_query, pane_layout,
    query_is_searchable, reduce_search,
};
use crate::screens::details::DetailsView;
use crate::shortcuts::Shortcut;

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

    let mut state = use_signal(SearchState::<MediaVm>::default);
    let mut selected = use_signal(|| None::<String>);

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

    let dual = pane_layout((ui.width)()) == PaneLayout::Dual;
    let query = state.read().query.clone();
    let placeholder = t("search.placeholder");
    let hint = t("search.hint");
    let clear_label = t("search.clear");

    let results = match &state.read().results {
        None => rsx! {
            EmptyState { icon: "search".to_string(), title: t("search.empty.title"), body: t("search.empty.body") }
        },
        Some(LoadState::Loading) => rsx! { SkeletonGrid { count: 12 } },
        Some(LoadState::Failed(e)) => {
            let q = state.peek().query.clone();
            rsx! {
                ErrorState { error: e.clone(), on_retry: move |_| run_search.call(q.clone()) }
            }
        }
        Some(LoadState::Ready(items)) if items.is_empty() => rsx! {
            EmptyState {
                icon: "search".to_string(),
                title: t("search.no_results.title"),
                body: tf("search.no_results.body", &[("query", &query)]),
            }
        },
        Some(LoadState::Ready(items)) => rsx! {
            div { class: "tk-grid", "data-dual": "{dual}",
                for m in items.clone() {
                    {
                        let id = m.id.clone();
                        let is_sel = selected.read().as_deref() == Some(id.as_str());
                        rsx! {
                            MediaCard {
                                key: "{m.id}",
                                media: m.clone(),
                                selected: is_sel,
                                onclick: move |_| {
                                    if dual {
                                        selected.set(Some(id.clone()));
                                    } else {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    }
                                },
                            }
                        }
                    }
                }
            }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--search", "data-dual": "{dual}",
            div { class: "tk-search__list",
                div { class: "tk-searchbar",
                    Icon { name: "search".to_string(), size: 22 }
                    input {
                        id: "tk-search-input",
                        class: "tk-searchbar__input",
                        r#type: "search",
                        placeholder: "{placeholder}",
                        "aria-label": "{placeholder}",
                        title: "{hint}",
                        autofocus: true,
                        autocomplete: "off",
                        spellcheck: "false",
                        value: "{query}",
                        oninput: move |e| run_search.call(e.value()),
                    }
                    if !query.is_empty() {
                        button {
                            class: "tk-iconbtn",
                            r#type: "button",
                            "aria-label": "{clear_label}",
                            title: "{clear_label}",
                            onclick: move |_| {
                                apply(&mut state, SearchAction::Typed(String::new()));
                                selected.set(None);
                            },
                            Icon { name: "close".to_string(), size: 20 }
                        }
                    }
                }
                {results}
            }
            if dual {
                aside { class: "tk-search__detail",
                    if let Some(sel) = selected.read().clone() {
                        DetailsView { key: "{sel}", id: sel.clone(), embedded: true }
                    } else {
                        EmptyState { icon: "film".to_string(), title: t("search.select_prompt") }
                    }
                }
            }
        }
    }
}
