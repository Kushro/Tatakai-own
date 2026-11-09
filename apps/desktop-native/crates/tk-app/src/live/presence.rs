//! `Backend::presence` (wave A, worker **e**; docs/redesign/15 section 4): forwards to `crate::discord` when `Settings::flow.discord_presence` is on (read with `settings_store::load(&b.settings_file)`), otherwise clears and returns `Ok(())`. Never an error for the UI once implemented: Discord not running = silent no-op with a debug log. Contract stub.

use tk_ui::backend::UiResult;
use tk_ui::model::UiError;
use tk_ui::backend::PresenceVm;
use crate::live::LiveBackend;

/// `Backend::presence`.
pub async fn presence(_b: &LiveBackend, _state: &PresenceVm) -> UiResult<()> {
    Err(UiError::not_wired("presence"))
}
