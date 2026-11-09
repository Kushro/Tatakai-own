//! Keyboard shortcuts (pure). The web view forwards `keydown` as `(key, ctrl, shift)`; this maps it to
//! an action given the current context. Player-only keys never fire outside the player, and plain
//! keys never fire while typing in a text field (the bridge reports `editable`).
//!
//! Wave A (worker **e**, docs/redesign/15-parity-wave-a-contract.md section 4): the player actions become
//! rebindable through `Settings::flow.shortcuts` ([`shortcut_in`]); `app.rs` already calls `shortcut_in` with the
//! user's map. The new variants below are final; `shortcut_in` is a stub that ignores the map until e lands.

use serde::Deserialize;

use crate::model::ShortcutMapVm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shortcut {
    ToggleNav,
    FocusSearch,
    TogglePause,
    SeekBack,
    SeekForward,
    Fullscreen,
    /// Ctrl+1..Ctrl+6 -> 1-based preset index.
    Preset(u8),
    Escape,
    /// 1..7 outside the player -> 1-based index into `NavDest::ALL` (7 destinations since wave 3).
    NavTo(u8),
    // --- wave A (worker e): player actions of the classic `keybindings.ts` (defaults in `default_binding`) ---
    /// `J` / `K` in the classic app.
    PrevEpisode,
    NextEpisode,
    /// `M`
    ToggleMute,
    /// `P`
    TogglePip,
    /// `S`: screenshot (worker d's `Player::screenshot_to_file`).
    Screenshot,
    /// `ArrowUp` / `ArrowDown`
    VolumeUp,
    VolumeDown,
    /// `>` / `<`
    SpeedUp,
    SpeedDown,
    /// `C` / `V`: next subtitle / audio track.
    CycleSub,
    CycleAudio,
    /// `Z` / `X`: subtitle delay -/+ 100 ms.
    SubDelayEarlier,
    SubDelayLater,
    /// `Enter` while the skip button shows.
    SkipSegment,
    /// `L`
    ToggleLoop,
}

impl Shortcut {
    /// Player actions the user can rebind (Settings > Player > Shortcuts), in display order.
    pub const REBINDABLE: [Shortcut; 19] = [
        Self::TogglePause,
        Self::SeekBack,
        Self::SeekForward,
        Self::Fullscreen,
        Self::PrevEpisode,
        Self::NextEpisode,
        Self::ToggleMute,
        Self::TogglePip,
        Self::Screenshot,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::SpeedUp,
        Self::SpeedDown,
        Self::CycleSub,
        Self::CycleAudio,
        Self::SubDelayEarlier,
        Self::SubDelayLater,
        Self::SkipSegment,
        Self::ToggleLoop,
    ];

    /// Stable id: key of `ShortcutMapVm` and i18n suffix `player.shortcut.<id>`.
    pub fn id(&self) -> &'static str {
        match self {
            Self::ToggleNav => "toggle_nav",
            Self::FocusSearch => "focus_search",
            Self::TogglePause => "toggle_pause",
            Self::SeekBack => "seek_back",
            Self::SeekForward => "seek_forward",
            Self::Fullscreen => "fullscreen",
            Self::Preset(_) => "preset",
            Self::Escape => "escape",
            Self::NavTo(_) => "nav_to",
            Self::PrevEpisode => "prev_episode",
            Self::NextEpisode => "next_episode",
            Self::ToggleMute => "toggle_mute",
            Self::TogglePip => "toggle_pip",
            Self::Screenshot => "screenshot",
            Self::VolumeUp => "volume_up",
            Self::VolumeDown => "volume_down",
            Self::SpeedUp => "speed_up",
            Self::SpeedDown => "speed_down",
            Self::CycleSub => "cycle_sub",
            Self::CycleAudio => "cycle_audio",
            Self::SubDelayEarlier => "sub_delay_earlier",
            Self::SubDelayLater => "sub_delay_later",
            Self::SkipSegment => "skip_segment",
            Self::ToggleLoop => "toggle_loop",
        }
    }
}

/// Default key spec (`KeyboardEvent.key`, optionally `Ctrl+` prefixed) of a rebindable action; `None` for actions
/// that are not rebindable. Worker e fills the wave-A defaults (classic `keybindings.ts`); the contract keeps the
/// bindings that exist today.
pub fn default_binding(action: &Shortcut) -> Option<&'static str> {
    match action {
        Shortcut::TogglePause => Some(" "),
        Shortcut::SeekBack => Some("ArrowLeft"),
        Shortcut::SeekForward => Some("ArrowRight"),
        Shortcut::Fullscreen => Some("f"),
        _ => None,
    }
}

/// [`shortcut`] with the user's rebinds applied (worker e). Contract stub: ignores `map` and keeps today's fixed
/// bindings, so behaviour is unchanged until e implements the lookup (`default_binding` + `map`).
pub fn shortcut_in(map: &ShortcutMapVm, key: &str, ctrl: bool, editable: bool, in_player: bool) -> Option<Shortcut> {
    let _ = map;
    shortcut(key, ctrl, editable, in_player)
}

/// Messages posted from the injected script to Rust.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t")]
pub enum JsMsg {
    #[serde(rename = "env")]
    Env { tag: String, dark: bool, w: f64 },
    #[serde(rename = "key")]
    Key {
        key: String,
        ctrl: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        editable: bool,
        /// The player screen is showing (the bridge checks `.tk-shell[data-player="true"]`).
        #[serde(default)]
        player: bool,
    },
}

pub fn shortcut(key: &str, ctrl: bool, editable: bool, in_player: bool) -> Option<Shortcut> {
    if ctrl {
        return match key.to_ascii_lowercase().as_str() {
            "b" => Some(Shortcut::ToggleNav),
            "k" => Some(Shortcut::FocusSearch),
            d @ ("1" | "2" | "3" | "4" | "5" | "6") if in_player => d.parse().ok().map(Shortcut::Preset),
            _ => None,
        };
    }
    if key == "Escape" {
        return Some(Shortcut::Escape);
    }
    if editable {
        return None;
    }
    if !in_player {
        return match key {
            "1" | "2" | "3" | "4" | "5" | "6" | "7" => key.parse().ok().map(Shortcut::NavTo),
            _ => None,
        };
    }
    match key {
        " " => Some(Shortcut::TogglePause),
        "ArrowLeft" => Some(Shortcut::SeekBack),
        "ArrowRight" => Some(Shortcut::SeekForward),
        "f" | "F" => Some(Shortcut::Fullscreen),
        _ => None,
    }
}

/// The listener installed in the web view. `dioxus.send` delivers each object to `JsMsg`.
pub const BRIDGE_JS: &str = r#"
const mq = window.matchMedia('(prefers-color-scheme: dark)');
const env = () => dioxus.send({ t: 'env', tag: navigator.language || 'en', dark: mq.matches, w: window.innerWidth });
env();
mq.addEventListener('change', env);
window.addEventListener('resize', env);
window.addEventListener('keydown', (e) => {
  const el = e.target;
  const editable = !!el && (el.tagName === 'INPUT' && el.type !== 'range' || el.tagName === 'TEXTAREA' || el.isContentEditable);
  const inPlayer = !!document.querySelector('.tk-shell[data-player="true"]');
  if (inPlayer && !editable && (e.key === ' ' || e.key === 'ArrowLeft' || e.key === 'ArrowRight')) e.preventDefault();
  if (e.ctrlKey && ['b', 'k', '1', '2', '3', '4', '5', '6'].includes(e.key.toLowerCase())) e.preventDefault();
  dioxus.send({ t: 'key', key: e.key, ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey, editable, player: inPlayer });
});
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_shortcuts() {
        assert_eq!(shortcut("b", true, false, false), Some(Shortcut::ToggleNav));
        assert_eq!(shortcut("B", true, true, false), Some(Shortcut::ToggleNav));
        assert_eq!(shortcut("k", true, false, true), Some(Shortcut::FocusSearch));
        assert_eq!(shortcut("Escape", false, true, false), Some(Shortcut::Escape));
    }

    #[test]
    fn player_keys_only_in_player_and_not_while_typing() {
        assert_eq!(shortcut(" ", false, false, true), Some(Shortcut::TogglePause));
        assert_eq!(shortcut(" ", false, false, false), None);
        assert_eq!(shortcut(" ", false, true, true), None);
        assert_eq!(shortcut("ArrowLeft", false, false, true), Some(Shortcut::SeekBack));
        assert_eq!(shortcut("ArrowRight", false, false, true), Some(Shortcut::SeekForward));
        assert_eq!(shortcut("f", false, false, true), Some(Shortcut::Fullscreen));
    }

    #[test]
    fn presets_map_ctrl_digits_in_the_player() {
        for n in 1..=6u8 {
            assert_eq!(shortcut(&n.to_string(), true, false, true), Some(Shortcut::Preset(n)));
        }
        assert_eq!(shortcut("1", true, false, false), None);
        assert_eq!(shortcut("7", true, false, true), None);
        assert_eq!(shortcut("0", true, false, true), None);
    }

    #[test]
    fn digits_navigate_outside_the_player_only() {
        assert_eq!(shortcut("1", false, false, false), Some(Shortcut::NavTo(1)));
        assert_eq!(shortcut("6", false, false, false), Some(Shortcut::NavTo(6)));
        assert_eq!(shortcut("7", false, false, false), Some(Shortcut::NavTo(7)));
        assert_eq!(shortcut("8", false, false, false), None);
        assert_eq!(shortcut("2", false, true, false), None);
        assert_eq!(shortcut("2", false, false, true), None);
    }

    #[test]
    fn rebind_stub_matches_fixed_bindings_and_ids_are_unique() {
        let map = ShortcutMapVm::default();
        for (key, ctrl, player) in [(" ", false, true), ("f", false, true), ("b", true, false), ("3", false, false), ("3", true, true)] {
            assert_eq!(shortcut_in(&map, key, ctrl, false, player), shortcut(key, ctrl, false, player));
        }
        let ids: std::collections::BTreeSet<_> = Shortcut::REBINDABLE.iter().map(Shortcut::id).collect();
        assert_eq!(ids.len(), Shortcut::REBINDABLE.len());
        assert_eq!(default_binding(&Shortcut::TogglePause), Some(" "));
        assert_eq!(default_binding(&Shortcut::Escape), None);
    }

    #[test]
    fn bridge_messages_deserialize() {
        let env: JsMsg = serde_json::from_str(r#"{"t":"env","tag":"es-AR","dark":true,"w":1280}"#).unwrap();
        assert_eq!(env, JsMsg::Env { tag: "es-AR".into(), dark: true, w: 1280.0 });
        let key: JsMsg = serde_json::from_str(r#"{"t":"key","key":"b","ctrl":true}"#).unwrap();
        assert_eq!(key, JsMsg::Key { key: "b".into(), ctrl: true, shift: false, editable: false, player: false });
    }
}
