//! Titles of a studio / producer (wave A, worker **a**; classic `/search/producer/:name`):
//! `Backend::search_by_studio(name, page)` as an infinite grid (shared [`PagedGrid`] of Browse). Route
//! `/studio/:name`; the studio chips of Details (worker c) link here with [`studio_route`].

use dioxus::prelude::*;

use crate::app::Route;
use crate::components::{EmptyState, IconButton};
use crate::i18n::{t, tf};
use crate::screens::browse::{PageSource, PagedGrid};

/// The route of a studio page (`name` as AniList spells it, e.g. `"MAPPA"`).
pub fn studio_route(name: &str) -> Route {
    Route::StudioPage { name: name.trim().to_owned() }
}

#[component]
pub fn StudioPage(name: String) -> Element {
    let navigator = use_navigator();
    let name = name.trim().to_owned();
    let back = rsx! {
        IconButton {
            icon: "back".to_string(),
            label: t("common.back"),
            onclick: move |_| {
                navigator.go_back();
            },
        }
    };
    if name.is_empty() {
        return rsx! {
            div { class: "tk-page tk-page--browse tk-studio",
                div { class: "tk-page__head", {back} h1 { class: "tk-page__title", {t("studio.title")} } }
                EmptyState { icon: "info".to_string(), title: t("studio.unknown.title"), body: t("studio.unknown.body") }
            }
        };
    }
    rsx! {
        div { class: "tk-page tk-page--browse tk-studio", "data-studio": "{name}",
            div { class: "tk-page__head",
                {back}
                div { class: "tk-studio__head",
                    span { class: "tk-studio__kicker tk-muted", {t("studio.title")} }
                    h1 { class: "tk-page__title", "{name}" }
                }
            }
            // Keyed by name: another studio remounts the grid (it loads once per mount).
            {std::iter::once(name.clone()).map(|n| {
                let source = PageSource::Studio(n.clone());
                let origin = format!("studio:{n}");
                rsx! {
                    PagedGrid {
                        key: "{n}",
                        source,
                        origin,
                        empty_title: t("studio.empty.title"),
                        empty_body: tf("studio.empty.body", &[("name", &n)]),
                        on_open: move |id: String| {
                            navigator.push(Route::DetailsPage { id });
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

    #[test]
    fn studio_route_trims_the_name() {
        assert_eq!(studio_route("  MAPPA "), Route::StudioPage { name: "MAPPA".into() });
    }
}
