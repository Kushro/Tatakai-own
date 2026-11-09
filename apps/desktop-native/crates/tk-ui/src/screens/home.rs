//! Home: greeting, hero carousel, continue watching and feed rows (prototype `screens.js::home`).
//! Each row degrades on its own: a failed row shows an inline `ErrorState` whose Retry reloads only that
//! row. Extension rows (`SectionVm::title_key` = `ext:<extension name>`, the `customHome` row) and rows
//! below the first few mount their content only when scrolled into view.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{HomeVm, MediaVm, SectionVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ContinueCard, EmptyState, ErrorState, HeroCarousel, InfoChip, MediaCard,
    SkeletonRow,
};
use crate::i18n::{t, tf};
use crate::model::{ErrorKind, UiError};

/// `title_key` prefix of an extension row; the rest is the extension's display name.
pub const EXT_ROW_PREFIX: &str = "ext:";
/// Hero slides taken from the first section.
const HERO_ITEMS: usize = 5;
/// Rows rendered right away; the rest wait until visible.
const EAGER_ROWS: usize = 3;
/// A lazy row mounts anyway after this long (in case the visibility event never arrives).
const LAZY_FALLBACK_MS: u64 = 6000;
/// The local hour for the greeting (the Rust side only knows UTC).
const HOUR_JS: &str = "dioxus.send(new Date().getHours());";

#[component]
pub fn HomePage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut home = use_resource(move || {
        let backend = backend.clone();
        async move { backend.home().await }
    });

    let body = match &*home.read() {
        None => rsx! {
            HomeSkeleton {}
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| home.restart() }
        },
        Some(Ok(vm)) => rsx! {
            HomeContent { vm: vm.clone(), on_retry: move |_| home.restart() }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--home",
            HomeHead {}
            {body}
        }
    }
}

/// Greeting by local time of day + the active metadata sources.
#[component]
fn HomeHead() -> Element {
    let ui = use_context::<Ui>();
    let mut hour = use_signal(|| None::<u32>);
    use_future(move || async move {
        let mut eval = document::eval(HOUR_JS);
        if let Ok(h) = eval.recv::<u32>().await {
            hour.set(Some(h));
        }
    });
    let title = match hour() {
        Some(h) if h < 12 => t("home.greeting.morning"),
        Some(h) if h < 19 => t("home.greeting.afternoon"),
        Some(_) => t("home.greeting.evening"),
        None => t("nav.home"),
    };
    // The first (preferred) source gets a check mark.
    let providers: Vec<(usize, String, String)> = ui
        .settings
        .read()
        .provider_priority
        .iter()
        .enumerate()
        .map(|(i, p)| (i, t(&format!("provider.{p}")), if i == 0 { "check".to_string() } else { String::new() }))
        .collect();
    rsx! {
        div { class: "tk-page__head tk-home__greet",
            div {
                h1 { class: "tk-page__title", "{title}" }
                p { class: "tk-page__intro", {t("search.hint")} }
            }
            div { class: "tk-home__sources", "aria-label": t("home.sources"),
                for (i , name , icon) in providers {
                    InfoChip { key: "{i}", text: name, icon }
                }
            }
        }
    }
}

#[component]
fn HomeSkeleton() -> Element {
    rsx! {
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--hero", "aria-hidden": "true" }
        }
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--line tk-home__skel-title", "aria-hidden": "true" }
            SkeletonRow { count: 8 }
        }
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--line tk-home__skel-title", "aria-hidden": "true" }
            SkeletonRow { count: 8 }
        }
    }
}

#[component]
fn HomeContent(vm: HomeVm, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    let hero: Vec<MediaVm> = vm
        .sections
        .first()
        .and_then(|s| s.items.as_ref().ok())
        .map(|items| items.iter().take(HERO_ITEMS).cloned().collect())
        .unwrap_or_default();
    let hero_key = hero.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(",");
    // The first section feeds the hero (as in the prototype); it is still listed if the hero is empty.
    let rows: Vec<SectionVm> = vm.sections.iter().skip(usize::from(!hero.is_empty())).cloned().collect();
    // A failed row still counts: it shows its own error.
    let has_rows = rows.iter().any(|s| !matches!(&s.items, Ok(items) if items.is_empty()));

    if hero.is_empty() && vm.continue_watching.is_empty() && !has_rows {
        return rsx! {
            EmptyState {
                icon: "film".to_string(),
                title: t("home.empty.title"),
                body: t("home.empty.body"),
                action_label: t("common.retry"),
                on_action: move |_| on_retry.call(()),
            }
        };
    }

    rsx! {
        if !hero.is_empty() {
            section { class: "tk-section",
                HeroCarousel {
                    key: "{hero_key}",
                    items: hero.clone(),
                    kicker: t("feed.trending"),
                    on_open: move |id: String| {
                        navigator.push(Route::DetailsPage { id });
                    },
                }
            }
        }
        if !vm.continue_watching.is_empty() {
            section { class: "tk-section",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title", {t("home.continue")} }
                    Button {
                        label: t("common.see_all"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::LibraryPage {});
                        },
                    }
                }
                div { class: "tk-row",
                    for c in vm.continue_watching.clone() {
                        {
                            let (id, episode) = (c.media.id.clone(), c.episode);
                            rsx! {
                                ContinueCard {
                                    key: "{c.media.id}",
                                    item: c.clone(),
                                    onclick: move |_| {
                                        navigator.push(Route::PlayerPage { id: id.clone(), episode });
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
        for (i , s) in rows.into_iter().enumerate() {
            {
                let lazy = i >= EAGER_ROWS || s.title_key.starts_with(EXT_ROW_PREFIX);
                rsx! {
                    SectionRow { key: "{s.title_key}", section: s.clone(), lazy }
                }
            }
        }
    }
}

/// One feed row. Retry re-runs `Backend::home()` and keeps only this row's result, so the other rows
/// never flash. `lazy` rows show a skeleton until they scroll into view.
#[component]
fn SectionRow(section: SectionVm, #[props(default)] lazy: bool) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut seen = use_signal(|| !lazy);
    let retried = use_signal(|| None::<Result<Vec<MediaVm>, UiError>>);
    let retrying = use_signal(|| false);

    use_future(move || async move {
        if *seen.peek() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(LAZY_FALLBACK_MS)).await;
        if !*seen.peek() {
            seen.set(true);
        }
    });

    let title_key = section.title_key.clone();
    let retry = move |_: ()| {
        let backend = services.backend.clone();
        let key = title_key.clone();
        let (mut retried, mut retrying) = (retried, retrying);
        retrying.set(true);
        spawn(async move {
            let items = match backend.home().await {
                Ok(vm) => vm
                    .sections
                    .into_iter()
                    .find(|s| s.title_key == key)
                    .map(|s| s.items)
                    .unwrap_or_else(|| Ok(Vec::new())),
                Err(e) => Err(e),
            };
            retried.set(Some(items));
            retrying.set(false);
        });
    };

    let ext_name = section.title_key.strip_prefix(EXT_ROW_PREFIX).map(str::to_owned);
    let title = match &ext_name {
        Some(name) => tf("home.ext_row", &[("name", name)]),
        None => t(&section.title_key),
    };
    let is_ext = ext_name.is_some();
    let feed = section.title_key.strip_prefix("feed.").unwrap_or(&section.title_key).to_owned();
    let items = retried().unwrap_or_else(|| section.items.clone());
    let busy = !seen() || retrying();

    let body = if busy {
        rsx! {
            SkeletonRow { count: 6 }
        }
    } else {
        match items {
            Ok(list) if list.is_empty() => rsx! {
                p { class: "tk-section__empty tk-muted", {t("home.empty.title")} }
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
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    },
                                }
                            }
                        }
                    }
                }
            },
            Err(e) => {
                let cause = if e.kind == ErrorKind::RateLimited { t("home.row_error") } else { String::new() };
                rsx! {
                    ErrorState { error: e, compact: true, cause, on_retry: retry }
                }
            }
        }
    };

    rsx! {
        section {
            class: "tk-section",
            "aria-busy": "{busy}",
            onvisible: move |e| {
                if !*seen.peek() && e.is_intersecting().unwrap_or(false) {
                    seen.set(true);
                }
            },
            div { class: "tk-section__head",
                h2 { class: "tk-section__title",
                    "{title}"
                    if is_ext {
                        small { class: "tk-section__count tk-mono", ".kai" }
                    }
                }
                if !is_ext {
                    Button {
                        label: t("common.see_all"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::BrowsePage { feed: feed.clone() });
                        },
                    }
                }
            }
            {body}
        }
    }
}
