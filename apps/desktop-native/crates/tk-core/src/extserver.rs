//! Client of a paired extension server (wave B, worker **j**; docs/redesign/13 section 3.2 is the API; checklist
//! `ext.server-client.*`, `ext.ecosystems.via-server`; classic `core/extensions/server/client.ts`). UI-free; tk-app's
//! `live/ext_server.rs` / `live/ext_catalog.rs` own the state. Same surface the Android `:core:extension-server`
//! client has (docs/redesign/13 section 3.10), so both apps talk to `server/` and to `tk-server` alike.
//!
//! Final public shape (j implements over `crate::net::SafeHttp` or a plain `reqwest::Client` with the token header
//! `Authorization: Bearer <token>`):
//! `pub struct ExtServerClient { base: Url, token: Option<String> }`,
//! `health() -> Health` (`GET /health`: `protocolVersion`, `serverId`, `name`, `platform`, `pairingRequired`),
//! `pair(code, label) -> Token` (`POST /auth/pair`), `login_viewer(password, label) -> Token` (`POST /auth/login`),
//! `logout()` (`POST /auth/logout`), `sources() -> Vec<SourceDescriptor>` (`GET /sources`),
//! `invoke(source_id, op, args) -> serde_json::Value` (`POST /invoke`),
//! `resolve(source_id, args) -> Vec<RemoteStream>` (`POST /resolve`, NDJSON progressive: one JSON object per line,
//! the last with `done: true`). Errors: `crate::error::ExtServerError` (`Refused(code)` for the 401 catalogue).

pub const API_PREFIX: &str = "/api/server/v1";

/// `GET /health` body (fields the client needs).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Health {
    pub protocol_version: u32,
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub pairing_required: bool,
}

/// Token answer of `/auth/pair` and `/auth/login`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub token: String,
    pub role: String,
    pub device_id: String,
}
