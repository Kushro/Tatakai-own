//! Keyboard shortcuts (pure). The web view forwards `keydown` as `(key, ctrl, shift)`; this maps it to
//! an action given the current context. Player-only keys never fire outside the player, and plain
//! keys never fire while typing in a text field (the bridge reports `editable`).
//!
//! Wave A (worker **e**, docs/redesign/15-parity-wave-a-contract.md section 4): the player actions are
//! rebindable through `Settings::flow.shortcuts` ([`shortcut_in`]; `app.rs` calls it with the user's map), with
//! the defaults of the classic `keybindings.ts` plus the extra player keys ([`default_binding`]). A key the user
//! assigned wins over another action's default; Settings > Player > Shortcuts marks such clashes ([`conflicts`]).
//! Key specs are `KeyboardEvent.key` values (`" "`, `"ArrowLeft"`, `"k"`), optionally prefixed with `Ctrl+`.

use std::collections::BTreeSet;

use serde::Deserialize;

use crate::i18n::t;
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
    /// `1`..`9` and `0` (= 10) outside the player -> 1-based index into `NavDest::ALL` (10 destinations since wave B).
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
/// that are not rebindable. Classic `keybindings.ts` (Space, arrows, F, M, P, J / K) plus the player extras.
pub fn default_binding(action: &Shortcut) -> Option<&'static str> {
    Some(match action {
        Shortcut::TogglePause => " ",
        Shortcut::SeekBack => "ArrowLeft",
        Shortcut::SeekForward => "ArrowRight",
        Shortcut::Fullscreen => "f",
        Shortcut::PrevEpisode => "j",
        Shortcut::NextEpisode => "k",
        Shortcut::ToggleMute => "m",
        Shortcut::TogglePip => "p",
        Shortcut::Screenshot => "s",
        Shortcut::VolumeUp => "ArrowUp",
        Shortcut::VolumeDown => "ArrowDown",
        Shortcut::SpeedUp => ">",
        Shortcut::SpeedDown => "<",
        Shortcut::CycleSub => "c",
        Shortcut::CycleAudio => "v",
        Shortcut::SubDelayEarlier => "z",
        Shortcut::SubDelayLater => "x",
        Shortcut::SkipSegment => "Enter",
        Shortcut::ToggleLoop => "l",
        Shortcut::ToggleNav
        | Shortcut::FocusSearch
        | Shortcut::Preset(_)
        | Shortcut::Escape
        | Shortcut::NavTo(_) => return None,
    })
}

/// The rebindable action with that id (`Shortcut::id`).
pub fn rebindable_by_id(id: &str) -> Option<Shortcut> {
    Shortcut::REBINDABLE.into_iter().find(|a| a.id() == id)
}

/// The key that triggers `action` now: the user's rebind, else the default.
pub fn binding(map: &ShortcutMapVm, action: &Shortcut) -> Option<String> {
    map.get(action.id()).map(str::to_owned).or_else(|| default_binding(action).map(str::to_owned))
}

/// `("Ctrl+", base)` split of a key spec.
fn split_spec(spec: &str) -> (bool, &str) {
    match spec.strip_prefix("Ctrl+") {
        Some(rest) if !rest.is_empty() => (true, rest),
        _ => (false, spec),
    }
}

/// Comparable form of a key name: Space is `" "`, single characters are lower-cased (`F` = `f`), named keys
/// compare case-insensitively.
fn norm_key(key: &str) -> String {
    if key == " " || key.eq_ignore_ascii_case("space") || key == "Spacebar" {
        return " ".to_owned();
    }
    if key.chars().count() == 1 { key.to_lowercase() } else { key.to_ascii_lowercase() }
}

fn norm_spec(spec: &str) -> (bool, String) {
    let (ctrl, base) = split_spec(spec);
    (ctrl, norm_key(base))
}

/// The pressed key (`KeyboardEvent.key` + Ctrl/Meta) is `spec`.
pub fn spec_matches(spec: &str, key: &str, ctrl: bool) -> bool {
    norm_spec(spec) == (ctrl, norm_key(key))
}

/// Key spec for a key captured in the editor; `None` for modifier-only and unidentified keys.
pub fn spec_from_event(key: &str, ctrl: bool) -> Option<String> {
    const IGNORED: [&str; 12] =
        ["Shift", "Control", "Alt", "AltGraph", "Meta", "OS", "Fn", "CapsLock", "Dead", "Unidentified", "Process", "NumLock"];
    if key.is_empty() || IGNORED.contains(&key) {
        return None;
    }
    let base = norm_key(key);
    // Named keys keep their `KeyboardEvent.key` spelling (`ArrowLeft`), characters are stored lower-case.
    let base = if base.chars().count() == 1 { base } else { key.to_owned() };
    Some(if ctrl { format!("Ctrl+{base}") } else { base })
}

/// Keys the editor refuses: they already mean something everywhere (Esc, `?` = shortcut panel, Tab = focus,
/// Ctrl+B / Ctrl+K, Ctrl+1..6 = upscaling presets).
pub fn is_reserved(spec: &str) -> bool {
    let (ctrl, base) = norm_spec(spec);
    if ctrl {
        return matches!(base.as_str(), "b" | "k" | "1" | "2" | "3" | "4" | "5" | "6");
    }
    matches!(base.as_str(), "escape" | "?" | "tab")
}

/// Ids of the rebindable actions whose current key is shared with another action. With a clash the user's
/// rebind wins (see [`shortcut_in`]), so the editor flags both rows.
pub fn conflicts(map: &ShortcutMapVm) -> BTreeSet<&'static str> {
    let keys: Vec<(&'static str, (bool, String))> = Shortcut::REBINDABLE
        .into_iter()
        .filter_map(|a| binding(map, &a).map(|spec| (a.id(), norm_spec(&spec))))
        .collect();
    let mut out = BTreeSet::new();
    for (i, (id, key)) in keys.iter().enumerate() {
        if keys.iter().enumerate().any(|(j, (_, other))| i != j && other == key) {
            out.insert(*id);
        }
    }
    out
}

/// `map` with `action` bound to `spec` (an explicit entry, even when it equals the default).
pub fn assign(map: &ShortcutMapVm, action: &Shortcut, spec: &str) -> ShortcutMapVm {
    let mut next = map.clone();
    next.0.insert(action.id().to_owned(), spec.to_owned());
    next.sanitized()
}

/// `map` with `action` back on its default key.
pub fn unassign(map: &ShortcutMapVm, action: &Shortcut) -> ShortcutMapVm {
    let mut next = map.clone();
    next.0.remove(action.id());
    next
}

/// Human-readable key: `Space`, arrows as glyphs, letters upper-case, `Ctrl+` kept.
pub fn display_spec(spec: &str) -> String {
    let (ctrl, base) = split_spec(spec);
    let shown = match norm_key(base).as_str() {
        " " => t("player.key.space"),
        "arrowleft" => "←".to_owned(),
        "arrowright" => "→".to_owned(),
        "arrowup" => "↑".to_owned(),
        "arrowdown" => "↓".to_owned(),
        "enter" => t("player.key.enter"),
        "escape" => "Esc".to_owned(),
        _ if base.chars().count() == 1 => base.to_uppercase(),
        _ => base.to_owned(),
    };
    if ctrl { format!("Ctrl+{shown}") } else { shown }
}

/// The player action for a key in the player: rebinds first, then the defaults of the actions that were not
/// rebound (so a rebind can take a key that is another action's default).
fn resolve_player(map: &ShortcutMapVm, key: &str, ctrl: bool) -> Option<Shortcut> {
    let rebound = Shortcut::REBINDABLE
        .into_iter()
        .find(|a| map.get(a.id()).is_some_and(|spec| spec_matches(spec, key, ctrl)));
    if rebound.is_some() {
        return rebound;
    }
    Shortcut::REBINDABLE.into_iter().find(|a| {
        map.get(a.id()).is_none() && default_binding(a).is_some_and(|spec| spec_matches(spec, key, ctrl))
    })
}

/// [`shortcut`] with the user's rebinds (`Settings::flow.shortcuts`). Global keys (Ctrl+B, Ctrl+K, Esc) and the
/// upscaling presets (Ctrl+1..6) are fixed; player actions only fire in the player and never while typing.
pub fn shortcut_in(map: &ShortcutMapVm, key: &str, ctrl: bool, editable: bool, in_player: bool) -> Option<Shortcut> {
    if ctrl {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "b" => return Some(Shortcut::ToggleNav),
            "k" => return Some(Shortcut::FocusSearch),
            "1" | "2" | "3" | "4" | "5" | "6" if in_player => return lower.parse().ok().map(Shortcut::Preset),
            _ => {}
        }
        return if in_player && !editable { resolve_player(map, key, true) } else { None };
    }
    if key == "Escape" {
        return Some(Shortcut::Escape);
    }
    if editable {
        return None;
    }
    if !in_player {
        return match key {
            // Wave B: ten rail destinations (`NavDest::ALL`): 1..=9, and `0` opens the tenth (Settings).
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => key.parse().ok().map(Shortcut::NavTo),
            "0" => Some(Shortcut::NavTo(10)),
            _ => None,
        };
    }
    resolve_player(map, key, false)
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

/// The default bindings (no rebinds).
pub fn shortcut(key: &str, ctrl: bool, editable: bool, in_player: bool) -> Option<Shortcut> {
    shortcut_in(&ShortcutMapVm::default(), key, ctrl, editable, in_player)
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
  if (inPlayer && !editable && [' ', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(e.key)) e.preventDefault();
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
        assert_eq!(shortcut("9", false, false, false), Some(Shortcut::NavTo(9)));
        assert_eq!(shortcut("0", false, false, false), Some(Shortcut::NavTo(10)));
        assert_eq!(shortcut("2", false, true, false), None);
        assert_eq!(shortcut("2", false, false, true), None);
    }

    #[test]
    fn defaults_cover_every_rebindable_action_with_unique_ids_and_keys() {
        let ids: std::collections::BTreeSet<_> = Shortcut::REBINDABLE.iter().map(Shortcut::id).collect();
        assert_eq!(ids.len(), Shortcut::REBINDABLE.len());
        for a in Shortcut::REBINDABLE {
            assert!(default_binding(&a).is_some(), "{} has no default", a.id());
            assert_eq!(rebindable_by_id(a.id()), Some(a.clone()));
        }
        assert!(conflicts(&ShortcutMapVm::default()).is_empty(), "the defaults never clash");
        assert_eq!(default_binding(&Shortcut::Escape), None);
        assert_eq!(rebindable_by_id("escape"), None);
    }

    #[test]
    fn classic_defaults_fire_in_the_player() {
        let cases = [
            ("m", Shortcut::ToggleMute),
            ("p", Shortcut::TogglePip),
            ("j", Shortcut::PrevEpisode),
            ("K", Shortcut::NextEpisode),
            ("ArrowUp", Shortcut::VolumeUp),
            ("ArrowDown", Shortcut::VolumeDown),
            (">", Shortcut::SpeedUp),
            ("<", Shortcut::SpeedDown),
            ("Enter", Shortcut::SkipSegment),
            ("l", Shortcut::ToggleLoop),
            ("s", Shortcut::Screenshot),
        ];
        for (key, action) in cases {
            assert_eq!(shortcut(key, false, false, true), Some(action), "{key}");
            assert_eq!(shortcut(key, false, false, false), None, "{key} outside the player");
        }
    }

    #[test]
    fn a_rebind_takes_over_the_key_and_frees_the_old_one() {
        let map = assign(&ShortcutMapVm::default(), &Shortcut::TogglePause, "k");
        assert_eq!(shortcut_in(&map, "k", false, false, true), Some(Shortcut::TogglePause));
        assert_eq!(shortcut_in(&map, " ", false, false, true), None, "space no longer pauses");
        // NextEpisode kept its default K, now shadowed: the editor flags both.
        let c = conflicts(&map);
        assert!(c.contains("toggle_pause") && c.contains("next_episode"));
        assert_eq!(c.len(), 2);
        // Reset one action / all.
        assert_eq!(shortcut_in(&unassign(&map, &Shortcut::TogglePause), " ", false, false, true), Some(Shortcut::TogglePause));
        // Ctrl combinations can be bound too, but never the global ones.
        let map = assign(&ShortcutMapVm::default(), &Shortcut::Screenshot, "Ctrl+s");
        assert_eq!(shortcut_in(&map, "s", true, false, true), Some(Shortcut::Screenshot));
        assert_eq!(shortcut_in(&map, "s", false, false, true), None);
        assert_eq!(shortcut_in(&map, "b", true, false, true), Some(Shortcut::ToggleNav));
        assert_eq!(shortcut_in(&map, "3", true, false, true), Some(Shortcut::Preset(3)));
        assert_eq!(shortcut_in(&map, "s", true, true, true), None, "not while typing");
    }

    #[test]
    fn captured_keys_become_specs_and_reserved_ones_are_refused() {
        assert_eq!(spec_from_event("K", false).as_deref(), Some("k"));
        assert_eq!(spec_from_event(" ", false).as_deref(), Some(" "));
        assert_eq!(spec_from_event("ArrowLeft", true).as_deref(), Some("Ctrl+ArrowLeft"));
        assert_eq!(spec_from_event("Shift", false), None);
        assert_eq!(spec_from_event("", false), None);
        for spec in ["Escape", "?", "Tab", "Ctrl+b", "Ctrl+K", "Ctrl+4"] {
            assert!(is_reserved(spec), "{spec}");
        }
        for spec in ["k", " ", "Ctrl+s", "Ctrl+7", "ArrowUp"] {
            assert!(!is_reserved(spec), "{spec}");
        }
        assert!(spec_matches("Space", " ", false));
        assert!(spec_matches("f", "F", false));
        assert!(!spec_matches("Ctrl+f", "f", false));
        assert_eq!(display_spec("ArrowLeft"), "←");
        assert_eq!(display_spec("Ctrl+s"), "Ctrl+S");
        assert_eq!(display_spec("PageUp"), "PageUp");
    }

    #[test]
    fn bridge_messages_deserialize() {
        let env: JsMsg = serde_json::from_str(r#"{"t":"env","tag":"es-AR","dark":true,"w":1280}"#).unwrap();
        assert_eq!(env, JsMsg::Env { tag: "es-AR".into(), dark: true, w: 1280.0 });
        let key: JsMsg = serde_json::from_str(r#"{"t":"key","key":"b","ctrl":true}"#).unwrap();
        assert_eq!(key, JsMsg::Key { key: "b".into(), ctrl: true, shift: false, editable: false, player: false });
    }
}
