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
//! * `GET /v1/events?wait=<1..=EVENTS_MAX_WAIT_SECS, default 25>` (PC -> phone, checklist `link.send-to-device`):
//!   long-poll per device. `200` + [`EventsPage`] as soon as the PC has events queued for the calling device
//!   (queued by `send_to_device`, drained on delivery, kept at most `EVENTS_QUEUE_MAX` per device and 10 min), or
//!   `204` when `wait` runs out with nothing. The phone polls while the app is in the foreground.
//! * Android mapping: `RemoteAction.PREV/PLAY_PAUSE/NEXT` -> `prev` / `toggle` / `next`;
//!   `handoff(animeId, episode, positionSec)` -> `open` with `mediaId = animeId`, `positionSecs = positionSec`;
//!   a [`PhoneEvent::Handoff`] -> `LinkRepository.pendingHandoff` (`LinkHandoff`) + local notification.

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

/// Longest `wait` of `GET /v1/events` (seconds); larger values are clamped.
pub const EVENTS_MAX_WAIT_SECS: u32 = 25;
/// Events kept per device until it polls (oldest dropped first).
pub const EVENTS_QUEUE_MAX: usize = 16;

/// One PC -> phone event of `GET /v1/events` (internally tagged by `type`, camelCase fields).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PhoneEvent {
    /// "Enviar a dispositivo": play the Link session `session_id` (already started by `send_to_device`) on the phone.
    #[serde(rename_all = "camelCase")]
    Handoff {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        media_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        episode: Option<u32>,
        #[serde(default)]
        position_secs: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
}

/// `GET /v1/events` `200` body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventsPage {
    pub events: Vec<PhoneEvent>,
}

/// Clamp the `wait` query parameter (absent = the maximum).
pub fn clamp_wait(wait: Option<u32>) -> u32 {
    wait.unwrap_or(EVENTS_MAX_WAIT_SECS).clamp(1, EVENTS_MAX_WAIT_SECS)
}

#[cfg(test)]
mod tests {
    use super::{clamp_wait, ControlCommand, EventsPage, PhoneEvent, PlayerState, EVENTS_MAX_WAIT_SECS};

    #[test]
    fn events_wire_is_type_tagged_and_camel_case() {
        let page = EventsPage { events: vec![PhoneEvent::Handoff { session_id: "s1".into(), media_id: Some("21".into()), episode: Some(3), position_secs: 75, title: None }] };
        assert_eq!(serde_json::to_string(&page).unwrap(), r#"{"events":[{"type":"handoff","sessionId":"s1","mediaId":"21","episode":3,"positionSecs":75}]}"#);
        let back: EventsPage = serde_json::from_str(r#"{"events":[{"type":"handoff","sessionId":"s2"}]}"#).unwrap();
        assert_eq!(back.events[0], PhoneEvent::Handoff { session_id: "s2".into(), media_id: None, episode: None, position_secs: 0, title: None });
        assert_eq!((clamp_wait(None), clamp_wait(Some(0)), clamp_wait(Some(99))), (EVENTS_MAX_WAIT_SECS, 1, EVENTS_MAX_WAIT_SECS));
    }

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
