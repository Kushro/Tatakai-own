//! The axum router of `/api/server/v1` (worker **k**; classic `server/src/core.mjs` `dispatch`). One handler per
//! route of [`ROUTES`]; everything answers through [`crate::api::ApiError`] on failure. Contract stub: [`router`]
//! returns a router whose only live route is `GET /health`; k adds the rest.
//!
//! Route table (method, path, who may call; `owner` = the app itself / admin token, `viewer` = paired phone,
//! `public` = no token):
//! * `GET /health` public -> `{ protocolVersion: 1, serverId, name, platform: "desktop", pairingRequired, dashboardPath: null }`
//! * `POST /setup` (`X-Tatakai-Bootstrap`) -> admin token (the app itself never needs it: it holds the state)
//! * `POST /auth/login { role, password, label? }` -> `{ token, role, deviceId }`; viewer without password: `401 pairing_required`
//! * `POST /auth/pair { code, label? }` -> `{ token, role: "viewer", deviceId }`; `401 pairing_expired`
//! * `POST /auth/logout` viewer|owner -> `{ success: true }`
//! * `POST /auth/pairing` owner -> `{ code, expiresIn: 300, url, deepLink }`; `409 pairing_limit`
//! * `POST /auth/password { password | null }` owner -> `{ success: true }`
//! * `GET /devices` owner -> `[{ id, label, role, createdAt, lastSeenAt, current }]`; `DELETE /devices/{id}` owner
//! * `GET /settings` owner / `POST /settings` owner (name, requirePairing, proxy)
//! * `GET /extensions` viewer -> the PC's `.kai` extensions `[{ id, name, version, type, sources: [...] }]` (`server.api.extensions`)
//! * `POST /extensions` owner -> install from URL (delegates to the app's installer); `DELETE /extensions/{id}` owner
//! * `GET /repositories` / `POST /repositories` owner (mirror of the app's repositories, read-only for phones)
//! * `GET /sources` viewer -> `SourceDescriptor[]` (`~read` / `~novel` variants) sorted by priority (`server.api.sources`)
//! * `POST /invoke { sourceId, op, args, progressive? }` viewer -> JSON or NDJSON (`server.api.invoke`)
//! * `POST /resolve { sourceId, media, episode }` viewer -> NDJSON progressive, last line `{ "done": true }` (`server.api.resolve-proxy`)
//! * `GET | HEAD /media/{ticket}/{*path}` viewer -> proxied media with the stored headers; `POST /media/refresh { ticket }`
//! * `GET /logs?after=` owner -> `{ entries, cursor }`; `GET /cache` owner -> `{ lookupEntries, mediaBytes }`; `DELETE /cache` owner
//! * `GET /export` owner / `POST /import` owner (settings + extension list, schema 1)
//! * `GET /diagnostics` owner -> `{ platform, protocolVersion, extensionCount, capabilities }`
//! * `POST /native` -> `404 not_found` (Node-only feature of the classic server; documented as unsupported)

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::API_PREFIX;
use crate::logs::LogBuffer;
use crate::state::ServerState;

/// `(method, path)` the router serves once k lands (paths relative to [`API_PREFIX`]).
pub const ROUTES: &[(&str, &str)] = &[
    ("GET", "/health"),
    ("POST", "/setup"),
    ("POST", "/auth/login"),
    ("POST", "/auth/pair"),
    ("POST", "/auth/logout"),
    ("POST", "/auth/pairing"),
    ("POST", "/auth/password"),
    ("GET", "/devices"),
    ("DELETE", "/devices/{id}"),
    ("GET", "/settings"),
    ("POST", "/settings"),
    ("GET", "/extensions"),
    ("POST", "/extensions"),
    ("DELETE", "/extensions/{id}"),
    ("GET", "/repositories"),
    ("POST", "/repositories"),
    ("GET", "/sources"),
    ("POST", "/invoke"),
    ("POST", "/resolve"),
    ("GET", "/media/{ticket}/{*path}"),
    ("HEAD", "/media/{ticket}/{*path}"),
    ("POST", "/media/refresh"),
    ("GET", "/logs"),
    ("GET", "/cache"),
    ("DELETE", "/cache"),
    ("GET", "/export"),
    ("POST", "/import"),
    ("GET", "/diagnostics"),
];

/// Shared handler state.
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<crate::ServerConfig>,
    pub ext: Arc<tk_ext::ExtensionHost>,
    pub state: Arc<ServerState>,
    pub logs: Arc<LogBuffer>,
}

/// `GET /health` (public). Final body shape; `name` falls back to the host name in k's version.
async fn health(axum::extract::State(st): axum::extract::State<AppState>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "protocolVersion": crate::PROTOCOL_VERSION,
        "serverId": st.state.server_id(),
        "name": st.cfg.name,
        "platform": "desktop",
        "pairingRequired": st.cfg.require_pairing && !st.state.viewer_password_set(),
        "dashboardPath": serde_json::Value::Null,
    }))
}

/// The router. Contract stub: only `/health`; k adds every route of [`ROUTES`] plus the CORS layer
/// (`tower_http::cors`, `cfg.allowed_origins`) and the request log (`logs`).
pub fn router(state: AppState) -> Router {
    Router::new().route(&format!("{API_PREFIX}/health"), get(health)).with_state(state)
}

#[cfg(test)]
mod tests {
    #[test]
    fn route_table_has_no_duplicates() {
        let mut seen = std::collections::BTreeSet::new();
        for r in super::ROUTES {
            assert!(seen.insert(*r), "duplicate route {r:?}");
        }
        assert!(super::ROUTES.contains(&("POST", "/invoke")));
    }
}
