//! Remote control over Tatakai Link (wave B, worker **k**; checklist `link.remote-control`, `link.handoff`).
//!
//! CANONICAL WIRE (shared with the Android client `core/link`, worker r; neither side changes it alone, same rule as
//! `/v1/history`; mTLS + device check like every route; k adds the handlers to `server.rs` and the entries to
//! `contracts/link/openapi.v1.json` through the integrator):
//! * `POST /v1/control` with a [`ControlCommand`]: `{ "command": "play" | "pause" | "toggle" | "next" | "prev" |
//!   "seek" | "open", "seconds"?: u32, "mediaId"?: string, "episode"?: u32, "positionSecs"?: u32 }` -> `204` when
//!   `LinkHost::control` took it, `409 no_player` when nothing is playing (`open` is always taken: the shell opens
//!   the player). Hand-off ("Continuar en la PC") is `open`: the phone pushes its history row first
//!   (`POST /v1/history`), so the PC resumes at `positionSecs`.
//! * `GET /v1/player` -> [`PlayerState`] (`LinkHost::player_state`), or `404 no_player` when the PC has no player
//!   open. There is no capability flag: a PC without remote control answers 404 too, and the phone then keeps the
//!   buttons disabled.
//! * Android mapping: `RemoteAction.PREV/PLAY_PAUSE/NEXT` -> `prev` / `toggle` / `next`;
//!   `handoff(animeId, episode, positionSec)` -> `open` with `mediaId = animeId`, `positionSecs = positionSec`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "lowercase")]
pub enum ControlCommand {
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    Seek {
        seconds: u32,
    },
    #[serde(rename_all = "camelCase")]
    Open {
        media_id: String,
        episode: u32,
        #[serde(default)]
        position_secs: u32,
    },
}

/// `GET /v1/player` answer: what the PC's player shows right now (camelCase on the wire).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerState {
    pub playing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode: Option<u32>,
    #[serde(default)]
    pub position_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ControlCommand, PlayerState};

    #[test]
    fn wire_form_is_camel_case_and_tagged() {
        let c: ControlCommand = serde_json::from_str(r#"{"command":"open","mediaId":"21","episode":3,"positionSecs":120}"#).unwrap();
        assert_eq!(c, ControlCommand::Open { media_id: "21".into(), episode: 3, position_secs: 120 });
        assert_eq!(serde_json::to_string(&ControlCommand::Seek { seconds: 9 }).unwrap(), r#"{"command":"seek","seconds":9}"#);
        assert_eq!(serde_json::to_string(&ControlCommand::Toggle).unwrap(), r#"{"command":"toggle"}"#);
        let st = PlayerState { playing: true, title: Some("x".into()), episode: Some(3), position_secs: 90, duration_secs: None, media_id: Some("21".into()) };
        assert_eq!(serde_json::to_string(&st).unwrap(), r#"{"playing":true,"title":"x","episode":3,"positionSecs":90,"mediaId":"21"}"#);
    }
}
