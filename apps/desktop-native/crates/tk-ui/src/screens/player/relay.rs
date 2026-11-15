//! Embedded enhanced + uosc (docs/redesign/09 section 4.4): pointer and keys over the video go to mpv, where the
//! build's input.conf and uosc handle them (right click = `script-binding uosc/menu`, wheel = volume, uosc's own
//! timeline / buttons / menus with the left button).
//!
//! One small page listener ([`RELAY_JS`]) on the hole sends compact messages over `dioxus.send`; [`run`] turns them
//! into mpv `mouse` / `keydown` / `keyup` commands. The page does the cheap work: coordinates in device pixels
//! (the same space as the GL surface mpv draws into, so uosc's hit boxes line up at any HiDPI scale), pointer moves
//! coalesced to one per [`MOVE_EVERY_MS`], wheel deltas summed into whole steps, key-ups that match their key-down
//! even when a modifier was released first, and everything released when the window loses focus. It is installed
//! only while the enhanced + uosc player is shown; a reinstall removes the previous listeners.
//!
//! mpv builds the double click (`MBTN_LEFT_DBL`) from two quick `MBTN_LEFT` downs itself, so it is not sent.

use std::sync::Arc;

use dioxus::prelude::*;
use serde::Deserialize;
use tk_player::{KeyAction, Player};

use crate::player_state::{mpv_key_name, mpv_mouse_button};

/// Pointer moves reach mpv at most this often (uosc redraws its hover state at about this rate anyway).
pub(super) const MOVE_EVERY_MS: u32 = 30;

/// Pointer movement re-flashes uosc's controls at most this often (its flash lasts 2 s).
const FLASH_EVERY_MS: u128 = 1000;

/// The page side. Messages: `{t:'m',x,y}` move, `{t:'d'|'u',x,y,b}` button down/up, `{t:'w',n,k}` wheel steps,
/// `{t:'l'}` pointer left, `{t:'kd'|'ku',k,c,a,s,m}` key down/up, `{t:'r'}` release everything.
pub(super) const RELAY_JS: &str = r#"
if (window.__tkRelayOff) window.__tkRelayOff();
const el = document.getElementById('tk-video-hole');
if (el) {
  const EVERY = __EVERY__;
  let pend = null, timer = 0, wy = 0, wx = 0, held = 0;
  const downs = new Map();
  const pt = (e) => {
    const r = el.getBoundingClientRect(), d = window.devicePixelRatio || 1;
    return { x: Math.max(0, Math.round((e.clientX - r.left) * d)), y: Math.max(0, Math.round((e.clientY - r.top) * d)) };
  };
  const flush = () => { if (timer) { clearTimeout(timer); timer = 0; } if (pend) { dioxus.send({ t: 'm', x: pend.x, y: pend.y }); pend = null; } };
  const move = (e) => { pend = pt(e); if (!timer) timer = setTimeout(() => { timer = 0; flush(); }, EVERY); };
  const down = (e) => {
    flush();
    const p = pt(e);
    held |= 1 << e.button;
    try { el.setPointerCapture(e.pointerId); } catch (_) {}
    if (document.activeElement !== el) el.focus({ preventScroll: true });
    dioxus.send({ t: 'd', x: p.x, y: p.y, b: e.button });
    e.preventDefault();
  };
  const up = (e) => {
    if (!(held & (1 << e.button))) return;
    flush();
    const p = pt(e);
    held &= ~(1 << e.button);
    dioxus.send({ t: 'u', x: p.x, y: p.y, b: e.button });
  };
  // Capture lost while a button is held (window switch, drag out of the web view): let every held button go.
  const upAll = (e) => {
    if (!held) return;
    const p = pt(e);
    for (let b = 0; b < 5; b++) if (held & (1 << b)) dioxus.send({ t: 'u', x: p.x, y: p.y, b });
    held = 0;
  };
  const leave = () => { if (!held) { flush(); dioxus.send({ t: 'l' }); } };
  const wheel = (e) => {
    e.preventDefault();
    const k = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1;
    wy += e.deltaY * k; wx += e.deltaX * k;
    const STEP = 40;
    const ny = Math.trunc(wy / STEP), nx = Math.trunc(wx / STEP);
    if (ny || nx) flush();
    if (ny) { wy -= ny * STEP; dioxus.send({ t: 'w', n: Math.min(Math.abs(ny), 5), k: ny < 0 ? 'WHEEL_UP' : 'WHEEL_DOWN' }); }
    if (nx) { wx -= nx * STEP; dioxus.send({ t: 'w', n: Math.min(Math.abs(nx), 5), k: nx < 0 ? 'WHEEL_LEFT' : 'WHEEL_RIGHT' }); }
  };
  const ours = (e) => e.key === 'Escape' || ((e.key === 'f' || e.key === 'F') && !e.ctrlKey && !e.altKey && !e.metaKey);
  const kd = (e) => {
    if (ours(e) || ['Control', 'Shift', 'Alt', 'Meta', 'AltGraph', 'CapsLock'].includes(e.key)) return;
    e.preventDefault();
    if (e.repeat) return; // mpv repeats a held key itself
    const m = { t: 'kd', k: e.key, c: e.ctrlKey, a: e.altKey, s: e.shiftKey, m: e.metaKey };
    downs.set(e.code || e.key, m);
    dioxus.send(m);
  };
  const ku = (e) => {
    const m = downs.get(e.code || e.key);
    if (!m) return;
    downs.delete(e.code || e.key);
    dioxus.send(Object.assign({}, m, { t: 'ku' }));
  };
  const release = () => { held = 0; downs.clear(); pend = null; dioxus.send({ t: 'r' }); };
  const ctx = (e) => e.preventDefault();
  const dbl = (e) => e.preventDefault();
  const on = [
    [el, 'pointermove', move], [el, 'pointerdown', down], [el, 'pointerup', up], [el, 'pointercancel', release],
    [el, 'lostpointercapture', upAll], [el, 'pointerleave', leave],
    [el, 'wheel', wheel, { passive: false }], [el, 'keydown', kd], [el, 'keyup', ku], [el, 'contextmenu', ctx],
    [el, 'dblclick', dbl], [window, 'blur', release],
  ];
  for (const [t, n, f, o] of on) t.addEventListener(n, f, o);
  window.__tkRelayOff = () => { for (const [t, n, f, o] of on) t.removeEventListener(n, f, o); if (timer) clearTimeout(timer); window.__tkRelayOff = null; };
  el.focus({ preventScroll: true });
}
"#;

/// One message from [`RELAY_JS`].
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub(super) struct RelayMsg {
    pub t: String,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    /// `MouseEvent.button`.
    #[serde(default)]
    pub b: u8,
    /// Wheel steps.
    #[serde(default)]
    pub n: u32,
    /// `KeyboardEvent.key`, or the wheel key name.
    #[serde(default)]
    pub k: String,
    #[serde(default)]
    pub c: bool,
    #[serde(default)]
    pub a: bool,
    #[serde(default)]
    pub s: bool,
    #[serde(default)]
    pub m: bool,
}

/// What one message asks mpv to do (pure; [`apply`] runs it).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RelayStep {
    /// `mouse <x> <y>` in device pixels of the video surface.
    Move(f64, f64),
    /// Move there, then `keydown` / `keyup` of the button.
    Button(f64, f64, &'static str, KeyAction),
    /// `keypress` of a wheel key, this many times.
    Wheel(&'static str, u32),
    Key(String, KeyAction),
    Leave,
    ReleaseAll,
}

pub(super) fn step(msg: &RelayMsg) -> Option<RelayStep> {
    Some(match msg.t.as_str() {
        "m" => RelayStep::Move(msg.x, msg.y),
        "d" | "u" => {
            let action = if msg.t == "d" { KeyAction::Down } else { KeyAction::Up };
            RelayStep::Button(msg.x, msg.y, mpv_mouse_button(msg.b)?, action)
        }
        "w" => {
            let key = match msg.k.as_str() {
                "WHEEL_UP" => "WHEEL_UP",
                "WHEEL_DOWN" => "WHEEL_DOWN",
                "WHEEL_LEFT" => "WHEEL_LEFT",
                "WHEEL_RIGHT" => "WHEEL_RIGHT",
                _ => return None,
            };
            RelayStep::Wheel(key, msg.n.clamp(1, 5))
        }
        "kd" | "ku" => {
            let action = if msg.t == "kd" { KeyAction::Down } else { KeyAction::Up };
            RelayStep::Key(mpv_key_name(&msg.k, msg.c, msg.a, msg.s, msg.m)?, action)
        }
        "l" => RelayStep::Leave,
        "r" => RelayStep::ReleaseAll,
        _ => return None,
    })
}

fn apply(p: &Player, s: RelayStep) {
    // Best effort: a refused command (no file yet) must not stop the relay.
    let _ = match s {
        RelayStep::Move(x, y) => p.forward_mouse(x, y, 1.0),
        RelayStep::Button(x, y, name, action) => {
            let _ = p.forward_mouse(x, y, 1.0);
            p.forward_key(name, action)
        }
        RelayStep::Wheel(name, n) => (0..n).try_for_each(|_| p.forward_key(name, KeyAction::Press)),
        RelayStep::Key(name, action) => p.forward_key(&name, action),
        RelayStep::Leave => p.forward_mouse_leave(),
        RelayStep::ReleaseAll => p.release_keys(),
    };
}

/// Install the page listener and forward its messages to `p` until the screen goes away. Spawn it only for the
/// enhanced + uosc player.
pub(super) async fn run(p: Arc<Player>) {
    let js = RELAY_JS.replace("__EVERY__", &MOVE_EVERY_MS.to_string());
    let mut eval = document::eval(&js);
    let mut flashed: Option<std::time::Instant> = None;
    while let Ok(msg) = eval.recv::<RelayMsg>().await {
        let Some(s) = step(&msg) else { continue };
        // Any movement brings uosc's timeline and controls up (it only reveals them near the bottom edge itself).
        if matches!(s, RelayStep::Move(..)) && flashed.is_none_or(|t| t.elapsed().as_millis() > FLASH_EVERY_MS) {
            flashed = Some(std::time::Instant::now());
            let _ = p.flash_uosc_controls();
        }
        apply(&p, s);
    }
    // The screen unmounted (or a newer relay replaced this one): nothing must stay pressed in mpv.
    let _ = p.release_keys();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(json: &str) -> RelayMsg {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn pointer_messages_map_to_mpv_mouse_and_buttons() {
        assert_eq!(step(&msg(r#"{"t":"m","x":960,"y":540}"#)), Some(RelayStep::Move(960.0, 540.0)));
        assert_eq!(
            step(&msg(r#"{"t":"d","x":10,"y":20,"b":2}"#)),
            Some(RelayStep::Button(10.0, 20.0, "MBTN_RIGHT", KeyAction::Down))
        );
        assert_eq!(step(&msg(r#"{"t":"u","x":1,"y":2,"b":0}"#)), Some(RelayStep::Button(1.0, 2.0, "MBTN_LEFT", KeyAction::Up)));
        assert_eq!(step(&msg(r#"{"t":"d","b":1}"#)), Some(RelayStep::Button(0.0, 0.0, "MBTN_MID", KeyAction::Down)));
        assert_eq!(step(&msg(r#"{"t":"d","b":9}"#)), None);
        assert_eq!(step(&msg(r#"{"t":"l"}"#)), Some(RelayStep::Leave));
        assert_eq!(step(&msg(r#"{"t":"r"}"#)), Some(RelayStep::ReleaseAll));
        assert_eq!(step(&msg(r#"{"t":"zz"}"#)), None);
    }

    #[test]
    fn wheel_steps_are_bounded_and_only_known_keys_pass() {
        assert_eq!(step(&msg(r#"{"t":"w","n":3,"k":"WHEEL_UP"}"#)), Some(RelayStep::Wheel("WHEEL_UP", 3)));
        assert_eq!(step(&msg(r#"{"t":"w","n":40,"k":"WHEEL_DOWN"}"#)), Some(RelayStep::Wheel("WHEEL_DOWN", 5)));
        assert_eq!(step(&msg(r#"{"t":"w","n":0,"k":"WHEEL_LEFT"}"#)), Some(RelayStep::Wheel("WHEEL_LEFT", 1)));
        assert_eq!(step(&msg(r#"{"t":"w","n":1,"k":"quit"}"#)), None);
    }

    #[test]
    fn keys_use_mpv_names_with_modifiers() {
        assert_eq!(step(&msg(r#"{"t":"kd","k":" "}"#)), Some(RelayStep::Key("SPACE".into(), KeyAction::Down)));
        assert_eq!(step(&msg(r#"{"t":"ku","k":"s","c":true}"#)), Some(RelayStep::Key("Ctrl+s".into(), KeyAction::Up)));
        assert_eq!(step(&msg(r#"{"t":"kd","k":"ArrowRight","s":true}"#)), Some(RelayStep::Key("Shift+RIGHT".into(), KeyAction::Down)));
        assert_eq!(step(&msg(r#"{"t":"kd","k":"Unidentified"}"#)), None);
    }

    #[test]
    fn the_page_script_keeps_the_relay_rules() {
        let js = RELAY_JS.replace("__EVERY__", &MOVE_EVERY_MS.to_string());
        assert!(!js.contains("__EVERY__"));
        // No synthetic double click (mpv builds it), a native context menu never shows, old listeners go first.
        assert!(!js.contains("MBTN_LEFT_DBL"));
        assert!(js.contains("'contextmenu', ctx") && js.starts_with("\nif (window.__tkRelayOff)"));
        assert!(js.contains("devicePixelRatio") && js.contains("setPointerCapture"));
    }
}
