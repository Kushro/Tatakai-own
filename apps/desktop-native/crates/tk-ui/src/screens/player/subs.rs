//! Subtitles in the player (wave A, worker **d**; docs/redesign/15-parity-wave-a-contract.md section 4). Owned by
//! d: preferred subtitle / audio language on `PlayerEvent::Tracks` (the audit bug "preferred subtitle language
//! ignored"), secondary subtitle, style (`Settings::subs.style` -> `tk_player::SubStyle`), delay, external SRT /
//! ASS / VTT files, live and pre-translation (`Backend::translate_cues`) and the screenshot button
//! (`Backend::screenshot_path` + `Player::screenshot_to_file`).
//!
//! `mod.rs` (worker e) only calls the three seams below with these exact signatures; d replaces the bodies.

use std::sync::Arc;

use dioxus::prelude::*;
use tk_player::{Player, TrackInfo, TrackKind};

use super::flow::Flow;
use crate::i18n::t;
use crate::model::{SourcePrefsVm, SubtitlePrefs};

/// `Option<Arc<Player>>` as a component prop (`Player` has no `PartialEq`; identity is what matters).
#[derive(Clone, Default)]
pub struct PlayerHandle(pub Option<Arc<Player>>);

impl PartialEq for PlayerHandle {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }
}

/// Called by `mod.rs` on every `PlayerEvent::Tracks` with the track list mpv reported. Returns the
/// `(subtitle id, audio id)` the UI should show as selected (0 = off / none).
///
/// Contract stub: mirrors what mpv selected (today's behaviour). Worker d: pick the subtitle track whose `lang`
/// matches `sources.sub_lang` (then `secondary_lang` for `secondary-sid`) and the audio track matching
/// `sources.dub_lang`, apply them with `Player::set_sub_track` / `set_secondary_sub_track` / `set_audio_track`,
/// apply `subs.style` with `set_sub_style`, and return the ids actually selected. `player` is `None` in tests.
pub fn on_tracks(player: Option<&Arc<Player>>, list: &[TrackInfo], subs: &SubtitlePrefs, sources: &SourcePrefsVm) -> (i64, i64) {
    let _ = (player, subs, sources);
    let sel = |kind: TrackKind| list.iter().find(|tr| tr.kind == kind && tr.selected).map_or(0, |tr| tr.id);
    (sel(TrackKind::Sub), sel(TrackKind::Audio))
}

/// The screenshot button (`PlayerControls::on_capture`). Contract stub: flashes "not wired". Worker d:
/// `Backend::screenshot_path(title, episode)` -> `Player::screenshot_to_file(path, with_subs)` -> flash the file
/// name (`capture.saved`), errors flashed as text.
pub fn capture(ctx: &Flow, title: &str, episode: u32) {
    let _ = (title, episode);
    ctx.flash(t("error.not_wired"));
}

/// Sheet opened from the subtitles menu ("Subtitle style..."): style controls with live preview, delay
/// (-/+ 100 ms, reset), secondary subtitle, "Load file..." (SRT / ASS / SSA / VTT via `Player::add_subtitle`),
/// translation (provider from Settings, translate now / live). Contract stub: renders nothing.
#[component]
pub fn SubtitleSheet(
    open: bool,
    player: PlayerHandle,
    tracks: Vec<TrackInfo>,
    /// Selected subtitle track id (0 = off).
    sub_track: i64,
    #[props(default)] on_close: EventHandler<()>,
) -> Element {
    let _ = (player, tracks, sub_track, on_close);
    if !open {
        return rsx! {};
    }
    rsx! {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(id: i64, kind: TrackKind, lang: &str, selected: bool) -> TrackInfo {
        TrackInfo { id, kind, lang: Some(lang.to_owned()), title: None, selected }
    }

    /// Contract behaviour: mpv's own selection is reported (worker d replaces this with the preference test).
    #[test]
    fn stub_reports_mpv_selection() {
        let list = vec![tr(1, TrackKind::Sub, "en", false), tr(2, TrackKind::Sub, "es", true), tr(1, TrackKind::Audio, "ja", true)];
        let (sid, aid) = on_tracks(None, &list, &SubtitlePrefs::default(), &SourcePrefsVm::default());
        assert_eq!((sid, aid), (2, 1));
        assert_eq!(on_tracks(None, &[], &SubtitlePrefs::default(), &SourcePrefsVm::default()), (0, 0));
    }
}
