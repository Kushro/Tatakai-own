//! Extension-server client (wave B, worker **j**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist
//! `ext.server-client.*`, `ext.ecosystems.via-server`, `ext.deep-link.server`; classic
//! `core/extensions/server/client.ts`, `ExtensionServersSettings.tsx`). Contract stubs: j replaces every body.
//! HTTP client in `tk_core::extserver` (`/api/server/v1`: `health`, `auth/pair`, `auth/login`, `auth/logout`,
//! `sources`, `invoke`, `resolve` NDJSON); servers in `LocalStore` (`tk-core/src/store/ext.rs::ext_servers`), tokens in
//! `b.secrets` under `tk_core::secrets::keys::ext_server(id)`. Remote candidates reach the ranking through
//! `ext_catalog::extra_candidates` (same file family, j).

use tk_ui::backend::{ExtServerPairVm, ExtServerVm, ExtSourceVm, UiResult};
use tk_ui::model::UiError;

use crate::live::LiveBackend;

/// Start-up: `take_pending_link` is consumed by the Extensions page, nothing to do here yet; j may test the enabled
/// servers once (`ext.server-client.fallback` marks offline ones). No-op until j lands.
pub async fn on_start(_b: &LiveBackend) {}

pub async fn servers(_b: &LiveBackend) -> UiResult<Vec<ExtServerVm>> {
    Err(UiError::not_wired("ext_servers"))
}
pub async fn pair(_b: &LiveBackend, _req: &ExtServerPairVm) -> UiResult<ExtServerVm> {
    Err(UiError::not_wired("ext_server_pair"))
}
pub async fn set_enabled(_b: &LiveBackend, _id: &str, _enabled: bool) -> UiResult<()> {
    Err(UiError::not_wired("ext_server_set_enabled"))
}
pub async fn remove(_b: &LiveBackend, _id: &str) -> UiResult<()> {
    Err(UiError::not_wired("ext_server_remove"))
}
pub async fn test(_b: &LiveBackend, _id: &str) -> UiResult<ExtServerVm> {
    Err(UiError::not_wired("ext_server_test"))
}
pub async fn sources(_b: &LiveBackend, _id: &str) -> UiResult<Vec<ExtSourceVm>> {
    Err(UiError::not_wired("ext_server_sources"))
}
/// `Backend::take_pending_server_link`: implemented (pure hand-over of `crate::deeplink::take`, the
/// `tatakai://extensionserver/connect?...` link `main.rs` kept from the command line or a second instance).
pub async fn take_pending_link(_b: &LiveBackend) -> UiResult<Option<String>> {
    Ok(crate::deeplink::take())
}
