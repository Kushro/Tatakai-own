//! Native extension server, media proxy and FlareSolverr (wave B, worker **k**; docs/redesign/16-parity-wave-b-contract.md
//! section 4; checklist `server.*`, `server.cors-proxy-local`). Contract stubs: k replaces every body. The server is
//! `crates/tk-server` (axum, `/api/server/v1`, docs/redesign/13 section 3.2) hosted in-process over the same
//! `tk_ext::ExtensionHost` the app uses (`b.ext.host()`): phones get the PC's `.kai` extensions. State (server id,
//! device tokens, viewer password hash, settings) in `<data>/server/state.json` (`tk_server::state`); prefs in
//! `Settings::server`. The loopback media proxy (`/api/server/v1/media/*`) also serves `server.cors-proxy-local`
//! for mpv when a stream needs headers mpv cannot send.

use tk_ui::backend::{
    FlareSolverrVm, ProxyStatusVm, ServerDeviceVm, ServerLogPageVm, ServerPairingVm, ServerStatusVm, UiResult,
};
use tk_ui::model::{Settings, UiError};

use crate::live::LiveBackend;
use crate::paths::AppPaths;

/// The in-process server handle and its config. k fills it (`tokio::sync::Mutex<Option<tk_server::ServerHandle>>`).
pub struct ExtServer {
    #[allow(dead_code)]
    paths: AppPaths,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl ExtServer {
    pub fn new(paths: AppPaths, client: reqwest::Client) -> Self {
        Self { paths, client }
    }
}

/// Start-up: start `tk-server` when `Settings::server.enabled` (`server.native-mode`). No-op until k lands.
pub async fn on_start(_b: &LiveBackend) {}

/// `Settings::server` changed: restart on a new port / name, toggle the proxy, update FlareSolverr. No-op until k lands.
pub async fn on_settings(_b: &LiveBackend, _settings: &Settings) {}

pub async fn status(_b: &LiveBackend) -> UiResult<ServerStatusVm> {
    Err(UiError::not_wired("server_status"))
}
pub async fn start(_b: &LiveBackend) -> UiResult<ServerStatusVm> {
    Err(UiError::not_wired("server_start"))
}
pub async fn stop(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("server_stop"))
}
pub async fn open_pairing(_b: &LiveBackend) -> UiResult<ServerPairingVm> {
    Err(UiError::not_wired("server_open_pairing"))
}
pub async fn devices(_b: &LiveBackend) -> UiResult<Vec<ServerDeviceVm>> {
    Err(UiError::not_wired("server_devices"))
}
pub async fn revoke(_b: &LiveBackend, _device_id: &str) -> UiResult<()> {
    Err(UiError::not_wired("server_revoke"))
}
pub async fn set_viewer_password(_b: &LiveBackend, _password: Option<&str>) -> UiResult<()> {
    Err(UiError::not_wired("server_set_viewer_password"))
}
pub async fn logs(_b: &LiveBackend, _after: u64) -> UiResult<ServerLogPageVm> {
    Err(UiError::not_wired("server_logs"))
}
pub async fn proxy_status(_b: &LiveBackend) -> UiResult<ProxyStatusVm> {
    Err(UiError::not_wired("server_proxy_status"))
}
pub async fn clear_cache(_b: &LiveBackend) -> UiResult<()> {
    Err(UiError::not_wired("server_clear_cache"))
}
pub async fn flaresolverr_test(_b: &LiveBackend, _url: Option<&str>) -> UiResult<FlareSolverrVm> {
    Err(UiError::not_wired("flaresolverr_test"))
}
