//! Torrent session of the player screen (docs/redesign/10 section 5.5), moved verbatim out of `player.rs` by the
//! wave-A contract (docs/redesign/15 section 4; owner: worker **e** with `mod.rs` / `flow.rs`; worker **i** only
//! reads it). What `Backend::torrent_play` returned, the slot `use_drop` ends, the batch hand-over to the next
//! episode and the speed / peers status line.

use std::cell::{Cell, RefCell};

use crate::backend::{BackendRef, SourceVm, TorrentStatsVm};
use crate::i18n::tf;
use crate::model::ErrorKind;
use crate::player_state::{downloaded_fraction, format_rate};

/// The "torrent has no file for this episode" flash stays this much longer than a regular one.
pub(super) const NO_FILE_NOTICE_EXTRA_MS: u64 = 6000;

/// Watched past this share of the duration = completed (`Backend::torrent_playback_ended`).
pub(super) const TORRENT_COMPLETED: f64 = 0.9;
/// Downloaded ranges (`Backend::seek_marks`) are refreshed every this many stats ticks (1 s each).
pub(super) const SEEK_MARKS_EVERY: u64 = 3;

/// The torrent this screen streams from: what `Backend::torrent_play` returned plus the source it came from.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TorrentSession {
    pub(super) torrent_id: String,
    pub(super) file_index: usize,
    pub(super) file_name: String,
    /// More than one video file: "next episode" reuses the torrent.
    pub(super) batch: bool,
    /// The picker row it was played from (carried to the next episode of a batch).
    pub(super) source: SourceVm,
}

/// The session outside signals so `use_drop` can still end it (`torrent_playback_ended`).
#[derive(Default)]
pub(super) struct TorrentSlot {
    pub(super) session: RefCell<Option<TorrentSession>>,
    /// Last known `PlayerState::fraction`, for `completed`.
    pub(super) fraction: Cell<f64>,
    /// The torrent was handed on (batch next episode, "open in the torrent client"): leaving this screen does not
    /// end it.
    pub(super) handed_over: Cell<bool>,
}

impl TorrentSlot {
    /// Replace the session; a different torrent that was playing ends here (not completed: the user switched).
    pub(super) fn replace(&self, backend: &BackendRef, next: Option<TorrentSession>) {
        let old = self.session.replace(next);
        let now = self.session.borrow().as_ref().map(|s| s.torrent_id.clone());
        if let Some(old) = old
            && now.as_deref() != Some(old.torrent_id.as_str())
        {
            end_torrent(backend, old.torrent_id, false);
        }
    }

    pub(super) fn completed(&self) -> bool {
        self.fraction.get() >= TORRENT_COMPLETED
    }
}

/// A batch handed to the next episode's screen: that screen plays `torrent_id` again with the file of its episode
/// instead of resolving sources. Single UI thread, so a thread-local is enough (the route only carries id/episode).
pub(super) struct BatchNext {
    pub(super) id: String,
    pub(super) episode: u32,
    pub(super) torrent_id: String,
    pub(super) source: SourceVm,
}

thread_local! {
    pub(super) static BATCH_NEXT: RefCell<Option<BatchNext>> = const { RefCell::new(None) };
}

/// The batch handed to `(id, episode)`, if any. A hand-over meant for another screen is stale (the user went
/// elsewhere): its torrent session ends here.
pub(super) fn take_batch_next(backend: &BackendRef, id: &str, episode: u32) -> Option<BatchNext> {
    let next = BATCH_NEXT.with(|b| b.borrow_mut().take())?;
    if next.id == id && next.episode == episode {
        return Some(next);
    }
    end_torrent(backend, next.torrent_id, false);
    None
}

/// `Backend::torrent_playback_ended` in the background. Runs from `use_drop` too, hence `spawn_forever` (the
/// screen's scope is going away).
pub(super) fn end_torrent(backend: &BackendRef, torrent_id: String, completed: bool) {
    let backend = backend.clone();
    dioxus::dioxus_core::spawn_forever(async move {
        if let Err(e) = backend.torrent_playback_ended(&torrent_id, completed).await
            && e.kind != ErrorKind::NotWired
        {
            tracing::warn!(detail = %e.detail, "ending the torrent session failed");
        }
    });
}

/// The speed / peers / file line under the buffering status (empty without stats).
pub(super) fn torrent_line(stats: Option<&TorrentStatsVm>, have: &[(f64, f64)]) -> String {
    let Some(s) = stats else { return String::new() };
    let file = if have.is_empty() { s.progress } else { downloaded_fraction(have) };
    let pct = ((file.clamp(0.0, 1.0) * 100.0).floor() as u32).to_string();
    tf("player.torrent.status", &[("speed", &format_rate(s.down_bps)), ("peers", &s.peers.to_string()), ("pct", &pct)])
}
