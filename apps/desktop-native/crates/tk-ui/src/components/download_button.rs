//! Player "Download" button (wave B, worker **h**; checklist `downloads.player-button`; classic
//! `video/controls/DownloadButton.tsx`). Mounted by `screens/player/mod.rs` next to `PlayerControls` (sibling, class
//! `tk-player__download`, hidden with the controls): one click calls `Backend::download_enqueue(media_id, episode,
//! &DownloadRequestVm { source_id, .. })` (the source playing now) and confirms with a snackbar; torrent sources go
//! through `torrent_add_download` instead. `data-download-state="idle|queued|done"`. Props are final.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{DownloadRequestVm, DownloadVm};
use crate::components::{IconButton, IconButtonVariant, use_snackbar};
use crate::i18n::t;
use crate::model::downloads::DownloadStateVm;

/// What the button shows for an episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonState {
    Idle,
    Queued,
    Done,
}

impl ButtonState {
    pub fn id(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Queued => "queued",
            Self::Done => "done",
        }
    }
}

/// State from the queue rows of the episode (a completed row wins over an active one).
pub fn state_for(rows: &[DownloadVm], media_id: &str, episode: u32) -> ButtonState {
    let mine = rows.iter().filter(|d| d.media_id == media_id && d.episode == Some(episode));
    let mut state = ButtonState::Idle;
    for d in mine {
        match d.state {
            DownloadStateVm::Completed => return ButtonState::Done,
            s if s.is_active() => state = ButtonState::Queued,
            _ => {}
        }
    }
    state
}

/// `"<ext>|torrent|<label>"` source keys go through the torrent client.
pub fn is_torrent_source(source_id: &str) -> bool {
    source_id.split('|').nth(1) == Some("torrent")
}

#[component]
pub fn PlayerDownloadButton(
    media_id: String,
    episode: u32,
    /// `SourceVm::id` of the playing source; `None` while resolving (the button is disabled).
    #[props(default)] source_id: Option<String>,
    /// Follows the controls' visibility.
    #[props(default = true)] visible: bool,
) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    // Queue state of this episode (re-read when the episode changes).
    let current = use_resource(use_reactive((&media_id, &episode), {
        let backend = backend.clone();
        move |(id, ep)| {
            let backend = backend.clone();
            async move { backend.downloads().await.map(|rows| state_for(&rows, &id, ep)).unwrap_or(ButtonState::Idle) }
        }
    }));
    // Set by a click for this `(media, episode)`; wins over the resource until the episode changes.
    let mut clicked = use_signal(|| None::<(String, u32, ButtonState)>);
    let mut busy = use_signal(|| false);

    let open_downloads = use_callback(move |_: ()| {
        navigator.push(Route::DownloadsPage {});
    });

    let start = use_callback({
        let backend = backend.clone();
        let (id, source) = (media_id.clone(), source_id.clone());
        move |_: ()| {
            if busy() {
                return;
            }
            let Some(source) = source.clone() else { return };
            busy.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                let result = if is_torrent_source(&source) {
                    backend.torrent_add_download(&source, &id, Some(episode)).await.map(|_| ())
                } else {
                    let req = DownloadRequestVm { source_id: Some(source.clone()), ..DownloadRequestVm::default() };
                    backend.download_enqueue(&id, episode, &req).await.map(|_| ())
                };
                match result {
                    Ok(()) => {
                        clicked.set(Some((id, episode, ButtonState::Queued)));
                        snack.show_action(t("downloads.player.queued"), t("downloads.player.open"), open_downloads);
                    }
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "queueing the episode failed");
                        snack.show_action(t("downloads.player.failed"), t("downloads.player.open"), open_downloads);
                    }
                }
                busy.set(false);
            });
        }
    });

    let state = match clicked() {
        Some((id, ep, s)) if id == media_id && ep == episode => s,
        _ => current().unwrap_or(ButtonState::Idle),
    };
    let (icon, label) = match state {
        ButtonState::Idle => ("download", t("downloads.player.download")),
        ButtonState::Queued => ("sync", t("downloads.player.queued_label")),
        ButtonState::Done => ("check", t("downloads.player.done")),
    };
    let disabled = busy() || source_id.is_none() || state != ButtonState::Idle;
    rsx! {
        div {
            class: "tk-player__download",
            "data-download-state": state.id(),
            "data-visible": "{visible}",
            "aria-hidden": "{!visible}",
            IconButton {
                icon: icon.to_string(),
                label: label,
                variant: IconButtonVariant::Tonal,
                disabled: disabled,
                onclick: move |_| start.call(()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::downloads::DownloadKindVm;

    fn row(ep: u32, state: DownloadStateVm) -> DownloadVm {
        DownloadVm {
            id: i64::from(ep),
            media_id: "21".into(),
            title: "x".into(),
            poster: None,
            episode: Some(ep),
            chapter: None,
            kind: DownloadKindVm::Hls,
            state,
            progress: 0.0,
            bytes_done: 0,
            bytes_total: None,
            speed_bps: 0,
            eta_secs: None,
            error: None,
            path: None,
            added_unix: 0,
            completed_unix: None,
        }
    }

    #[test]
    fn button_state_from_rows() {
        let rows = vec![row(1, DownloadStateVm::Completed), row(2, DownloadStateVm::Paused), row(3, DownloadStateVm::Failed)];
        assert_eq!(state_for(&rows, "21", 1), ButtonState::Done);
        assert_eq!(state_for(&rows, "21", 2), ButtonState::Queued);
        assert_eq!(state_for(&rows, "21", 3), ButtonState::Idle);
        assert_eq!(state_for(&rows, "22", 1), ButtonState::Idle);
        assert!(is_torrent_source("nyaa|torrent|SubsPlease"));
        assert!(!is_torrent_source("ext|web|Alpha"));
        assert_eq!(ButtonState::Queued.id(), "queued");
    }
}
