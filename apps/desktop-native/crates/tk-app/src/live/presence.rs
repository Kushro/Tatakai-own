//! `Backend::presence` (wave A, worker **e**; docs/redesign/15 section 4): forwards to `crate::discord` when
//! `Settings::flow.discord_presence` is on (read with `settings_store::load(&b.settings_file)`), otherwise clears.
//! Never an error for the UI: Discord not running is a silent no-op (the worker thread logs at `debug` and retries).

use tk_ui::backend::{PresenceVm, UiResult};

use crate::live::LiveBackend;
use crate::settings_store;

/// What reaches Discord: the UI's state when the setting is on, `Clear` otherwise.
fn effective(enabled: bool, state: &PresenceVm) -> PresenceVm {
    if enabled { state.clone() } else { PresenceVm::Clear }
}

/// `Backend::presence`.
pub async fn presence(b: &LiveBackend, state: &PresenceVm) -> UiResult<()> {
    // A clear needs no settings read (the setting being turned off sends one at once).
    let target = if *state == PresenceVm::Clear {
        PresenceVm::Clear
    } else {
        effective(settings_store::load(&b.settings_file).flow.discord_presence, state)
    };
    crate::discord::global().update(&target);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_setting_gates_what_is_sent() {
        let s = PresenceVm::Viewing { title: "Frieren".into() };
        assert_eq!(effective(true, &s), s);
        assert_eq!(effective(false, &s), PresenceVm::Clear);
        assert_eq!(effective(true, &PresenceVm::Clear), PresenceVm::Clear);
    }
}
