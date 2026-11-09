//! Episode list over the player (wave A, worker **e**; checklist `player.jump-to-episode`, classic
//! `WatchPage.tsx`): "Episodes" in the top bar opens a sheet with the title's episodes (`Backend::episodes`),
//! the current one highlighted; picking one plays it with its best source without going back to the details page.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::Services;
use crate::components::{EmptyState, EpisodeRow, ErrorState, Sheet, SkeletonLines, Tone};
use crate::i18n::{t, tf};
use crate::model::SKELETON_DELAY_MS;

/// The sheet; renders nothing while closed (the list is fetched each time it opens).
#[component]
pub fn EpisodeSheet(
    open: bool,
    id: String,
    current: u32,
    #[props(default)] on_pick: EventHandler<u32>,
    #[props(default)] on_close: EventHandler<()>,
) -> Element {
    if !open {
        return rsx! {};
    }
    rsx! {
        Sheet { title: t("player.episodes"), wide: true, on_close: move |_| on_close.call(()),
            EpisodeList { id, current, on_pick }
        }
    }
}

#[component]
fn EpisodeList(id: String, current: u32, on_pick: EventHandler<u32>) -> Element {
    let services = use_context::<Services>();
    let mut list = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.episodes(&id).await }
        }
    });
    // The skeleton only shows when the answer takes longer than a blink.
    let mut slow = use_signal(|| false);
    use_future(move || async move {
        tokio::time::sleep(Duration::from_millis(SKELETON_DELAY_MS)).await;
        slow.set(true);
    });

    let state = list.read().clone();
    match state {
        None => rsx! {
            div { class: "tk-player-eps", "data-episodes": "loading", "aria-busy": "true",
                if slow() {
                    SkeletonLines { widths: vec![90, 80, 85, 70, 75] }
                }
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "tk-player-eps", "data-episodes": "error",
                ErrorState { error: e, compact: true, on_retry: move |_| list.restart() }
            }
        },
        Some(Ok(eps)) if eps.is_empty() => rsx! {
            div { class: "tk-player-eps", "data-episodes": "empty",
                EmptyState { icon: "film".to_string(), title: t("player.episodes_empty"), body: t("player.episodes_empty_body") }
            }
        },
        Some(Ok(eps)) => {
            let count = eps.len();
            rsx! {
                div { class: "tk-player-eps tk-list", "data-episodes": "{count}",
                    for ep in eps {
                        {
                            let n = ep.number;
                            let is_current = n == current;
                            let pills = if is_current { vec![(t("player.episodes_now"), Tone::Primary)] } else { Vec::new() };
                            let label = tf("player.episode", &[("n", &n.to_string())]);
                            rsx! {
                                EpisodeRow {
                                    key: "{n}",
                                    episode: ep.clone(),
                                    current: is_current,
                                    pills,
                                    sub: if ep.title.is_some() { label } else { String::new() },
                                    trailing_icon: if is_current { String::new() } else { "play".to_string() },
                                    onclick: move |_| {
                                        if !is_current {
                                            on_pick.call(n);
                                        }
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
