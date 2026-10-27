//! Keyboard shortcuts (pure). The web view forwards `keydown` as `(key, ctrl, shift)`; this maps it to
//! an action given the current context. Player-only keys never fire outside the player, and plain
//! keys never fire while typing in a text field (the bridge reports `editable`).

use serde::Deserialize;

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
}

/// Messages posted from the injected script to Rust.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t")]
pub enum JsMsg {
    #[serde(rename = "env")]
    Env { tag: String, dark: bool, w: f64 },
    #[serde(rename = "key")]
    Key { key: String, ctrl: bool, #[serde(default)] shift: bool, #[serde(default)] editable: bool },
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
    if editable || !in_player {
        return None;
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
  dioxus.send({ t: 'key', key: e.key, ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey, editable });
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
    fn bridge_messages_deserialize() {
        let env: JsMsg = serde_json::from_str(r#"{"t":"env","tag":"es-AR","dark":true,"w":1280}"#).unwrap();
        assert_eq!(env, JsMsg::Env { tag: "es-AR".into(), dark: true, w: 1280.0 });
        let key: JsMsg = serde_json::from_str(r#"{"t":"key","key":"b","ctrl":true}"#).unwrap();
        assert_eq!(key, JsMsg::Key { key: "b".into(), ctrl: true, shift: false, editable: false });
    }
}
