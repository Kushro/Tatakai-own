//! Details: header, synopsis, episodes grouped in ranges of 100. `DetailsView` is reused as the right
//! pane of Search on wide windows (multi-pane layout).

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{DetailsVm, EpisodeVm};
use crate::components::{
    Button, EmptyState, EpisodeRow, ErrorState, InfoChip, MediaCover, Segmented,
};
use crate::i18n::{t, tf};
use crate::model::{EPISODE_RANGE, WindowClass, episode_ranges, range_index_for, window_class};

#[component]
pub fn DetailsPage(id: String) -> Element {
    rsx! {
        div { class: "tk-page tk-page--details",
            DetailsView { key: "{id}", id: id.clone(), embedded: false }
        }
    }
}

#[component]
pub fn DetailsView(id: String, #[props(default)] embedded: bool) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let backend = services.backend.clone();

    let mut details = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.details(&id).await }
        }
    });
    let mut episodes = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.episodes(&id).await }
        }
    });

    let wide = window_class((ui.width)()) != WindowClass::Compact && !embedded;
    let layout = if wide { "split" } else { "stack" };

    let header = match &*details.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--header", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| details.restart() }
        },
        Some(Ok(vm)) => rsx! {
            DetailsHeader { vm: vm.clone(), id: id.clone() }
        },
    };

    let resume = details.read().as_ref().and_then(|r| r.as_ref().ok()).map(|d| d.resume_episode).unwrap_or(0);

    let list = match &*episodes.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--list", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| episodes.restart() }
        },
        Some(Ok(eps)) if eps.is_empty() => rsx! {
            EmptyState {
                icon: "film".to_string(),
                title: t("details.no_episodes.title"),
                body: t("details.no_episodes.body"),
            }
        },
        Some(Ok(eps)) => rsx! {
            EpisodeList { id: id.clone(), episodes: eps.clone(), resume }
        },
    };

    rsx! {
        div { class: "tk-details", "data-layout": "{layout}", "data-embedded": "{embedded}",
            div { class: "tk-details__info", {header} }
            div { class: "tk-details__episodes",
                h2 { class: "tk-section__title", {t("details.episodes")} }
                {list}
            }
        }
    }
}

#[component]
fn DetailsHeader(vm: DetailsVm, id: String) -> Element {
    let navigator = use_navigator();
    let watch_episode = if vm.resume_episode > 0 { vm.resume_episode } else { 1 };
    let watch_label = if vm.resume_episode > 0 {
        tf("details.continue", &[("n", &vm.resume_episode.to_string())])
    } else {
        t("details.watch")
    };
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("details.no_synopsis"));
    let count = vm.episodes_total.map(|n| tf("details.episodes_count", &[("n", &n.to_string())]));
    rsx! {
        div { class: "tk-dhead",
            div { class: "tk-dhead__cover",
                MediaCover { src: vm.media.cover.clone(), color: vm.media.color.clone(), title: vm.media.title.clone() }
            }
            div { class: "tk-dhead__text",
                h1 { class: "tk-dhead__title", "{vm.media.title}" }
                div { class: "tk-chips",
                    if !vm.media.subtitle.is_empty() {
                        InfoChip { text: vm.media.subtitle.clone() }
                    }
                    if let Some(status) = vm.status.clone() {
                        InfoChip { text: status }
                    }
                    if let Some(c) = count {
                        InfoChip { text: c }
                    }
                    if let Some(score) = vm.media.score {
                        InfoChip { text: tf("details.score", &[("n", &score.to_string())]) }
                    }
                    for g in vm.genres.clone() {
                        InfoChip { key: "{g}", text: g.clone() }
                    }
                }
                div { class: "tk-dhead__actions",
                    Button {
                        label: watch_label,
                        icon: "play".to_string(),
                        size: crate::components::ButtonSize::Lg,
                        onclick: move |_| {
                            navigator.push(Route::PlayerPage { id: id.clone(), episode: watch_episode });
                        },
                    }
                }
                h2 { class: "tk-section__title tk-section__title--sm", {t("details.synopsis")} }
                p { class: "tk-dhead__synopsis", "{synopsis}" }
            }
        }
    }
}

#[component]
fn EpisodeList(id: String, episodes: Vec<EpisodeVm>, resume: u32) -> Element {
    let navigator = use_navigator();
    let max = episodes.iter().map(|e| e.number).max().unwrap_or(0);
    let ranges = episode_ranges(max, EPISODE_RANGE);
    let mut range = use_signal(|| if resume > 0 { range_index_for(resume, EPISODE_RANGE) } else { 0 });
    let active = range().min(ranges.len().saturating_sub(1));
    let (from, to) = ranges.get(active).copied().unwrap_or((1, max));
    let shown: Vec<EpisodeVm> = episodes.iter().filter(|e| e.number >= from && e.number <= to).cloned().collect();
    let options: Vec<(String, String)> = ranges
        .iter()
        .enumerate()
        .map(|(i, (a, b))| (i.to_string(), tf("details.range", &[("from", &a.to_string()), ("to", &b.to_string())])))
        .collect();

    rsx! {
        if ranges.len() > 1 {
            Segmented {
                options,
                selected: active.to_string(),
                label: t("details.episodes"),
                on_select: move |v: String| range.set(v.parse().unwrap_or(0)),
            }
        }
        div { class: "tk-eps",
            for e in shown {
                {
                    let (id, n) = (id.clone(), e.number);
                    rsx! {
                        EpisodeRow {
                            key: "{n}",
                            episode: e.clone(),
                            current: n == resume,
                            onclick: move |_| {
                                navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                            },
                        }
                    }
                }
            }
        }
    }
}
