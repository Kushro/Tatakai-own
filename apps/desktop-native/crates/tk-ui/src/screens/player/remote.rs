//! Remote control over Tatakai Link (wave B, worker **k**; checklist `link.remote-control`, `link.handoff`):
//! `app.rs::Shell` long-polls `Backend::wait_remote_command` and turns each [`RemoteCommandVm`] into the player
//! shortcut the player already handles (`ShortcutEvt`), or navigates to the player for `Open`. Pure mapping here so
//! it is unit-tested without Dioxus.
//!
//! `Seek(target)` is absolute but the player only has relative seek shortcuts: the backend reports the player's
//! position here ([`note_position`], from `Backend::presence` / `record_progress`), and the seek becomes one
//! `SeekForward` / `SeekBack` step towards the target. `Play` / `Pause` arrive only when they change something
//! (the backend drops `play` while playing and `pause` while paused), so both are the toggle.

use std::sync::atomic::{AtomicU32, Ordering};

use crate::backend::RemoteCommandVm;
use crate::shortcuts::Shortcut;

/// Last position (seconds) the player reported; `u32::MAX` = unknown.
static POSITION: AtomicU32 = AtomicU32::new(u32::MAX);

/// Called by the backend whenever the player reports where it is.
pub fn note_position(secs: u32) {
    POSITION.store(secs, Ordering::Relaxed);
}

fn known_position() -> Option<u32> {
    Some(POSITION.load(Ordering::Relaxed)).filter(|p| *p != u32::MAX)
}

/// The relative step that moves `current` towards `target` (`None` when already there, within 2 s). Pure.
pub fn seek_step(target: u32, current: u32) -> Option<Shortcut> {
    if target > current.saturating_add(2) {
        Some(Shortcut::SeekForward)
    } else if target.saturating_add(2) < current {
        Some(Shortcut::SeekBack)
    } else {
        None
    }
}

/// The shortcut equivalent of a transport command; `None` for `Open` (routed by the shell) and for a seek whose
/// direction is unknown (no position reported yet) or already reached.
pub fn shortcut_for(cmd: &RemoteCommandVm) -> Option<Shortcut> {
    match cmd {
        RemoteCommandVm::Play | RemoteCommandVm::Pause | RemoteCommandVm::TogglePause => Some(Shortcut::TogglePause),
        RemoteCommandVm::NextEpisode => Some(Shortcut::NextEpisode),
        RemoteCommandVm::PrevEpisode => Some(Shortcut::PrevEpisode),
        RemoteCommandVm::Seek(target) => known_position().and_then(|p| seek_step(*target, p)),
        RemoteCommandVm::Open { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_commands_map_to_shortcuts() {
        assert_eq!(shortcut_for(&RemoteCommandVm::TogglePause), Some(Shortcut::TogglePause));
        assert_eq!(shortcut_for(&RemoteCommandVm::NextEpisode), Some(Shortcut::NextEpisode));
        assert_eq!(shortcut_for(&RemoteCommandVm::Open { media_id: "21".into(), episode: 1, position_secs: 0 }), None);
    }

    #[test]
    fn seeks_step_towards_the_target() {
        assert_eq!(seek_step(100, 50), Some(Shortcut::SeekForward));
        assert_eq!(seek_step(10, 50), Some(Shortcut::SeekBack));
        assert_eq!(seek_step(51, 50), None);
        note_position(50);
        assert_eq!(shortcut_for(&RemoteCommandVm::Seek(300)), Some(Shortcut::SeekForward));
    }
    // ---- F6 (parity wave B) ----
    // (POSITION is a process-wide static shared with the test above, so these stay on the pure paths.)

    #[test]
    fn play_pause_and_prev_map_like_the_classic_remote() {
        assert_eq!(shortcut_for(&RemoteCommandVm::Play), Some(Shortcut::TogglePause));
        assert_eq!(shortcut_for(&RemoteCommandVm::Pause), Some(Shortcut::TogglePause));
        assert_eq!(shortcut_for(&RemoteCommandVm::PrevEpisode), Some(Shortcut::PrevEpisode));
    }

    #[test]
    fn seek_tolerance_is_two_seconds_each_way() {
        assert_eq!(seek_step(52, 50), None);
        assert_eq!(seek_step(53, 50), Some(Shortcut::SeekForward));
        assert_eq!(seek_step(48, 50), None);
        assert_eq!(seek_step(47, 50), Some(Shortcut::SeekBack));
        assert_eq!(seek_step(0, 0), None);
        assert_eq!(seek_step(0, 3), Some(Shortcut::SeekBack));
        // no overflow at the extremes
        assert_eq!(seek_step(u32::MAX, u32::MAX - 1), None);
        assert_eq!(seek_step(u32::MAX - 1, u32::MAX), None);
        assert_eq!(seek_step(u32::MAX, 0), Some(Shortcut::SeekForward));
    }
}
