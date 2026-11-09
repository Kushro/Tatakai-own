//! Discord Rich Presence (wave A, worker **e**; docs/redesign/15 section 4) over `discord-rich-presence`
//! (`DiscordIpcClient::new(APP_ID)`, `connect`, `set_activity`, `clear_activity`, `close`): states browsing /
//! viewing <title> / watching <title> E<n> with timestamps / paused, poster as `large_image` URL. The client is
//! blocking: keep it in a `std::sync::Mutex` and call it from `tokio::task::spawn_blocking`; reconnect lazily
//! when Discord was not running (never log above `debug`). `APP_ID` is the Tatakai Discord application id used
//! by the classic app's `desktop/services/discord-rpc.cjs` (e copies it; it is a public client id). Contract stubs.

use tk_ui::backend::PresenceVm;

/// Set by worker e (public Discord application id, not a secret).
pub const APP_ID: &str = "";

/// Process-wide presence client. Contract stub: does nothing.
pub struct Presence;

impl Presence {
    pub fn new() -> Self {
        Self
    }

    /// Contract stub (worker e). Must not block the caller for long and must never fail the UI.
    pub fn update(&self, state: &PresenceVm) {
        let _ = state;
    }

    /// Clear and disconnect (app exit, setting turned off). Contract stub.
    pub fn shutdown(&self) {}
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}
