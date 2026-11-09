//! Remote control over Tatakai Link (wave B, worker **k**; checklist `link.remote-control`, `link.handoff`):
//! `app.rs::Shell` long-polls `Backend::wait_remote_command` and turns each [`RemoteCommandVm`] into the player
//! shortcut the player already handles (`ShortcutEvt`), or navigates to the player for `Open`. Pure mapping here so
//! it is unit-tested without Dioxus; k extends it (seek -> a dedicated event) without touching `player/mod.rs`.

use crate::backend::RemoteCommandVm;
use crate::shortcuts::Shortcut;

/// The shortcut equivalent of a transport command; `None` for commands the shortcut layer cannot express yet
/// (`Seek`: k adds `Ui::remote_seek` or a `Shortcut::SeekTo` in its own files; `Open` is routed by the shell).
pub fn shortcut_for(cmd: &RemoteCommandVm) -> Option<Shortcut> {
    match cmd {
        RemoteCommandVm::Play | RemoteCommandVm::Pause | RemoteCommandVm::TogglePause => Some(Shortcut::TogglePause),
        RemoteCommandVm::NextEpisode => Some(Shortcut::NextEpisode),
        RemoteCommandVm::PrevEpisode => Some(Shortcut::PrevEpisode),
        RemoteCommandVm::Seek(_) | RemoteCommandVm::Open { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_commands_map_to_shortcuts() {
        assert_eq!(shortcut_for(&RemoteCommandVm::TogglePause), Some(Shortcut::TogglePause));
        assert_eq!(shortcut_for(&RemoteCommandVm::NextEpisode), Some(Shortcut::NextEpisode));
        assert_eq!(shortcut_for(&RemoteCommandVm::Seek(30)), None);
        assert_eq!(shortcut_for(&RemoteCommandVm::Open { media_id: "21".into(), episode: 1, position_secs: 0 }), None);
    }
}
