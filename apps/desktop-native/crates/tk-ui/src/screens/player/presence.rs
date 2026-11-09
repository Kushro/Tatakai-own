//! Discord Rich Presence from the UI (wave A, worker **e**; docs/redesign/15 section 4). The player screen sends
//! `Watching` (title, episode, position, pause) on mount, on every pause change and every
//! [`WATCHING_EVERY_SECS`]; [`use_route_presence`] (called by the shell) sends `Browsing` on any other page and
//! `Viewing <title>` on a details page. `Backend::presence` gates everything with `Settings::flow.discord_presence`
//! and never fails the UI. The state is mirrored as `data-presence` on `.tk-shell` (`off` / `browsing` / `viewing`
//! / `watching`, plus `data-presence-paused`) so it can be checked without Discord.

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, DetailsVm, PlaybackVm, PresenceVm};
use crate::model::PlayerState;

/// The player refreshes its presence this often (position drift, Discord restarts).
pub const WATCHING_EVERY_SECS: u64 = 15;

/// `data-presence` value of a state.
pub fn presence_attr(state: &PresenceVm) -> &'static str {
    match state {
        PresenceVm::Clear => "off",
        PresenceVm::Browsing => "browsing",
        PresenceVm::Viewing { .. } => "viewing",
        PresenceVm::Watching { .. } => "watching",
    }
}

fn shell_attr_js(state: &PresenceVm) -> String {
    let paused = matches!(state, PresenceVm::Watching { paused: true, .. });
    format!(
        "(() => {{ const el = document.querySelector('.tk-shell'); if (el) {{ el.setAttribute('data-presence', '{}'); el.setAttribute('data-presence-paused', '{}'); }} }})()",
        presence_attr(state),
        paused
    )
}

/// Send `state` (fire and forget). Runs on the root scope, so it also works from `use_drop`.
pub fn send(backend: &BackendRef, state: PresenceVm) {
    let backend = backend.clone();
    dioxus::dioxus_core::spawn_forever(async move {
        let _ = document::eval(&shell_attr_js(&state));
        if let Err(e) = backend.presence(&state).await {
            tracing::debug!(kind = ?e.kind, detail = %e.detail, "presence update failed");
        }
    });
}

/// The player's presence: title and poster from the playing source (else the details), position and pause from mpv.
pub fn watching_state(meta: Option<&PlaybackVm>, info: Option<&DetailsVm>, st: &PlayerState, episode: u32) -> PresenceVm {
    let title = meta
        .map(|m| m.title.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| info.map(|d| d.media.title.clone()))
        .unwrap_or_default();
    let poster = meta.and_then(|m| m.poster.clone()).or_else(|| info.and_then(|d| d.media.cover.clone()));
    PresenceVm::Watching {
        title,
        episode,
        position_secs: st.progress_secs(),
        duration_secs: st.duration_secs(),
        paused: st.paused,
        poster,
    }
}

/// Browsing / viewing presence for the current route (the player sends its own). Call once from the shell
/// with the current route; it re-sends on every route change and when the setting changes (off -> `Clear`).
pub fn use_route_presence(route: Route) {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let backend = services.backend.clone();
    // Bumped on every run so a late details answer never overwrites a newer page's state.
    let mut generation = use_signal(|| 0_u64);
    use_effect(use_reactive((&route,), move |(route,)| {
        let enabled = ui.settings.read().flow.discord_presence;
        let loaded = (ui.settings_loaded)();
        if !loaded || route.is_player() {
            return;
        }
        let gen_now = *generation.peek() + 1;
        generation.set(gen_now);
        if !enabled {
            send(&backend, PresenceVm::Clear);
            return;
        }
        match route {
            Route::DetailsPage { id } => {
                let backend = backend.clone();
                spawn(async move {
                    let title = backend.details(&id).await.map(|d| d.media.title).unwrap_or_default();
                    if *generation.peek() != gen_now {
                        return;
                    }
                    let state = if title.is_empty() { PresenceVm::Browsing } else { PresenceVm::Viewing { title } };
                    send(&backend, state);
                });
            }
            _ => send(&backend, PresenceVm::Browsing),
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_name_each_state() {
        assert_eq!(presence_attr(&PresenceVm::Clear), "off");
        assert_eq!(presence_attr(&PresenceVm::Browsing), "browsing");
        assert_eq!(presence_attr(&PresenceVm::Viewing { title: "x".into() }), "viewing");
        let w = PresenceVm::Watching {
            title: "x".into(),
            episode: 1,
            position_secs: 0,
            duration_secs: None,
            paused: true,
            poster: None,
        };
        assert_eq!(presence_attr(&w), "watching");
        assert!(shell_attr_js(&w).contains("'data-presence-paused', 'true'"));
    }
}
