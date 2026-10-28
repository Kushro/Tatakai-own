//! Home: hero carousel, continue watching and feed rows. Each row degrades on its own.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ContinueVm, HomeVm, MediaVm, SectionVm};
use crate::components::{Button, EmptyState, ErrorState, MediaCard, MediaCover, SkeletonRow};
use crate::i18n::{t, tf};

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
            div { class: "tk-section",
                SkeletonRow { count: 8 }
            }
            div { class: "tk-section",
                SkeletonRow { count: 8 }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| home.restart() }
        },
        Some(Ok(vm)) => rsx! {
            HomeContent { vm: vm.clone(), on_retry: move |_| home.restart() }
        },
    };

    rsx! {
        div { class: "tk-page", {body} }
    }
}

#[component]
fn HomeContent(vm: HomeVm, on_retry: EventHandler<()>) -> Element {
    let hero: Vec<MediaVm> = vm
        .sections
        .first()
        .and_then(|s| s.items.as_ref().ok())
        .map(|items| items.iter().take(5).cloned().collect())
        .unwrap_or_default();
    let nothing = hero.is_empty() && vm.continue_watching.is_empty() && vm.sections.iter().all(|s| s.items.is_err());

    if nothing && vm.sections.is_empty() {
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
            section { class: "tk-hero", "aria-label": t("feed.trending"),
                for m in hero {
                    HeroSlide { key: "{m.id}", media: m.clone() }
                }
            }
        }
        if !vm.continue_watching.is_empty() {
            section { class: "tk-section",
                h2 { class: "tk-section__title", {t("home.continue")} }
                div { class: "tk-row",
                    for c in vm.continue_watching.clone() {
                        ContinueCard { key: "{c.media.id}", item: c.clone() }
                    }
                }
            }
        }
        for s in vm.sections.clone() {
            SectionRow { key: "{s.title_key}", section: s.clone(), on_retry }
        }
    }
}

#[component]
fn HeroSlide(media: MediaVm) -> Element {
    let navigator = use_navigator();
    let id = media.id.clone();
    rsx! {
        article { class: "tk-hero__slide",
            div { class: "tk-hero__cover",
                MediaCover { src: media.cover.clone(), color: media.color.clone(), title: media.title.clone() }
            }
            div { class: "tk-hero__text",
                h2 { class: "tk-hero__title", "{media.title}" }
                if !media.subtitle.is_empty() {
                    p { class: "tk-hero__sub", "{media.subtitle}" }
                }
                Button {
                    label: t("details.watch"),
                    icon: "play".to_string(),
                    onclick: move |_| {
                        navigator.push(Route::DetailsPage { id: id.clone() });
                    },
                }
            }
        }
    }
}

#[component]
fn ContinueCard(item: ContinueVm) -> Element {
    let navigator = use_navigator();
    let id = item.media.id.clone();
    let episode = item.episode;
    let badge = tf("home.continue_episode", &[("n", &episode.to_string())]);
    rsx! {
        MediaCard {
            media: item.media.clone(),
            progress: item.fraction(),
            badge,
            onclick: move |_| {
                navigator.push(Route::PlayerPage { id: id.clone(), episode });
            },
        }
    }
}

#[component]
fn SectionRow(section: SectionVm, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    let title = t(&section.title_key);
    rsx! {
        section { class: "tk-section",
            h2 { class: "tk-section__title", "{title}" }
            match &section.items {
                Ok(items) => rsx! {
                    div { class: "tk-row",
                        for m in items.clone() {
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
                Err(e) => rsx! {
                    ErrorState { error: e.clone(), compact: true, on_retry }
                },
            }
        }
    }
}
