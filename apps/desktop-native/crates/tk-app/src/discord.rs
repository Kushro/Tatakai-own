//! Discord Rich Presence (wave A, worker **e**; docs/redesign/15 section 4) over `discord-rich-presence`
//! (`DiscordIpcClient::new(APP_ID)`, `connect`, `set_activity`, `clear_activity`, `close`): states browsing /
//! viewing <title> / watching <title> E<n> with timestamps / paused, poster as `large_image` URL.
//!
//! The IPC client is blocking, so it lives on one worker thread fed by a channel: `update` only queues the wanted
//! state (never blocks the UI, keeps the order of the calls) and the thread applies the latest one. When Discord is
//! not running the thread retries every [`RETRY_SECS`] while something should be shown; nothing above `debug` is
//! logged. `APP_ID` is the Tatakai Discord application id the classic app passes to
//! `desktop/services/discord-rpc.cjs` (`desktop/main.cjs`); a public client id, not a secret (plan 14 decision
//! 2026-11-09). Texts come from the UI catalog (`player.discord.*`) in the current UI language.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use discord_rich_presence::activity::{Activity, ActivityType, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use tk_ui::backend::PresenceVm;
use tk_ui::i18n::{t, tf};

/// Public Discord application id of Tatakai (same as the classic app).
pub const APP_ID: &str = "1466113024929697836";

/// Reconnect attempts while Discord is closed and something should be shown.
const RETRY_SECS: u64 = 30;
/// Discord rejects `details` / `state` longer than 128 characters (and shorter than 2).
const MAX_TEXT: usize = 128;
/// Asset keys registered on the Tatakai application (classic `discord-rpc.cjs`).
const LOGO_KEY: &str = "logo";
const PLAY_KEY: &str = "play_icon";
const PAUSE_KEY: &str = "pause_icon";

enum Cmd {
    Update(PresenceVm),
    Shutdown,
}

/// Process-wide presence client (see [`global`]).
pub struct Presence {
    tx: Mutex<Option<Sender<Cmd>>>,
}

impl Presence {
    pub fn new() -> Self {
        Self { tx: Mutex::new(None) }
    }

    /// Queue `state`; the worker thread starts on first use. Never blocks for long and never fails the UI.
    pub fn update(&self, state: &PresenceVm) {
        let Ok(mut guard) = self.tx.lock() else { return };
        if guard.is_none() {
            if *state == PresenceVm::Clear {
                // Nothing shown yet and nothing to show: do not start a thread for it.
                return;
            }
            let (tx, rx) = mpsc::channel();
            let spawned = std::thread::Builder::new().name("discord-presence".to_owned()).spawn(move || worker(rx));
            match spawned {
                Ok(_) => *guard = Some(tx),
                Err(e) => {
                    tracing::debug!(error = %e, "discord presence thread did not start");
                    return;
                }
            }
        }
        if let Some(tx) = guard.as_ref()
            && tx.send(Cmd::Update(state.clone())).is_err()
        {
            // The worker is gone (it never panics on Discord errors, but be safe): start over next time.
            *guard = None;
        }
    }

    /// Clear and disconnect (app exit, setting turned off).
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.tx.lock()
            && let Some(tx) = guard.take()
        {
            let _ = tx.send(Cmd::Shutdown);
        }
    }
}

impl Default for Presence {
    fn default() -> Self {
        Self::new()
    }
}

/// The presence client of this process (one IPC connection for the whole app).
pub fn global() -> &'static Presence {
    static PRESENCE: OnceLock<Presence> = OnceLock::new();
    PRESENCE.get_or_init(Presence::new)
}

/// What one presence state shows in Discord (pure, unit-tested).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityParts {
    pub details: String,
    pub state: String,
    pub large_image: String,
    pub large_text: String,
    pub small_image: Option<&'static str>,
    pub small_text: Option<String>,
    /// Unix milliseconds (elapsed / remaining bar while watching).
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub watching: bool,
}

/// Discord limits: 2..=128 characters.
fn fit(text: String) -> String {
    let mut s: String = text.chars().take(MAX_TEXT).collect();
    while s.chars().count() < 2 {
        s.push(' ');
    }
    s
}

/// The activity for `state` at `now_ms`; `None` for [`PresenceVm::Clear`].
pub fn activity_parts(state: &PresenceVm, now_ms: i64) -> Option<ActivityParts> {
    let base = |details: String, state: String| ActivityParts {
        details: fit(details),
        state: fit(state),
        large_image: LOGO_KEY.to_owned(),
        large_text: "Tatakai".to_owned(),
        small_image: None,
        small_text: None,
        start_ms: None,
        end_ms: None,
        watching: false,
    };
    match state {
        PresenceVm::Clear => None,
        PresenceVm::Browsing => Some(base(t("player.discord.browsing"), t("player.discord.browsing_state"))),
        PresenceVm::Viewing { title } => {
            Some(base(tf("player.discord.viewing", &[("title", title)]), t("player.discord.viewing_state")))
        }
        PresenceVm::Watching { title, episode, position_secs, duration_secs, paused, poster } => {
            let ep = tf("player.discord.episode", &[("n", &episode.to_string())]);
            let details =
                if *paused { tf("player.discord.paused", &[("title", title)]) } else { tf("player.discord.watching", &[("title", title)]) };
            let mut parts = base(details, ep);
            parts.watching = true;
            if let Some(p) = poster.as_ref().filter(|p| p.starts_with("https://")) {
                parts.large_image = p.clone();
            }
            if !title.trim().is_empty() {
                parts.large_text = fit(title.clone());
            }
            if *paused {
                parts.small_image = Some(PAUSE_KEY);
                parts.small_text = Some(t("player.discord.paused_small"));
            } else {
                parts.small_image = Some(PLAY_KEY);
                parts.small_text = Some(t("player.discord.playing_small"));
                let pos = i64::from(*position_secs);
                parts.start_ms = Some(now_ms - pos * 1000);
                if let Some(d) = duration_secs.map(i64::from).filter(|d| *d > pos) {
                    parts.end_ms = Some(now_ms + (d - pos) * 1000);
                }
            }
            Some(parts)
        }
    }
}

fn to_activity(p: &ActivityParts) -> Activity<'_> {
    let mut assets = Assets::new().large_image(p.large_image.as_str()).large_text(p.large_text.as_str());
    if let Some(key) = p.small_image {
        assets = assets.small_image(key);
    }
    if let Some(text) = &p.small_text {
        assets = assets.small_text(text.as_str());
    }
    let mut a = Activity::new().details(p.details.as_str()).state(p.state.as_str()).assets(assets);
    if p.watching {
        a = a.activity_type(ActivityType::Watching);
    }
    if p.start_ms.is_some() || p.end_ms.is_some() {
        let mut ts = Timestamps::new();
        if let Some(s) = p.start_ms {
            ts = ts.start(s);
        }
        if let Some(e) = p.end_ms {
            ts = ts.end(e);
        }
        a = a.timestamps(ts);
    }
    a
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// The worker: owns the IPC client, applies the latest wanted state, reconnects lazily.
fn worker(rx: Receiver<Cmd>) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut wanted = PresenceVm::Clear;
    // `wanted` is on Discord (false after a failure, so the timeout retries it).
    let mut applied = true;
    loop {
        let cmd = if applied || wanted == PresenceVm::Clear {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(Duration::from_secs(RETRY_SECS)) {
                Ok(c) => Some(c),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        match cmd {
            Some(Cmd::Shutdown) => break,
            Some(Cmd::Update(state)) => {
                // Only the newest of a burst matters.
                let mut latest = state;
                let mut shutdown = false;
                while let Ok(more) = rx.try_recv() {
                    match more {
                        Cmd::Update(s) => latest = s,
                        Cmd::Shutdown => shutdown = true,
                    }
                }
                if shutdown {
                    break;
                }
                // Repeated non-watching states (route changes on the same page) change nothing on Discord.
                if latest == wanted && applied && !matches!(latest, PresenceVm::Watching { .. }) {
                    continue;
                }
                wanted = latest;
            }
            None => {}
        }
        applied = apply(&mut client, &wanted);
    }
    if let Some(mut c) = client.take() {
        let _ = c.clear_activity();
        let _ = c.close();
    }
}

/// Push `state` to Discord, connecting first when needed. `true` when Discord has it (or nothing was needed).
fn apply(client: &mut Option<DiscordIpcClient>, state: &PresenceVm) -> bool {
    let Some(parts) = activity_parts(state, now_ms()) else {
        // Clear: only a live connection has something to clear.
        if let Some(c) = client.as_mut()
            && let Err(e) = c.clear_activity()
        {
            tracing::debug!(error = %e, "discord: clearing the presence failed");
            if let Some(mut c) = client.take() {
                let _ = c.close();
            }
        }
        return true;
    };
    if client.is_none() {
        let mut c = DiscordIpcClient::new(APP_ID);
        if let Err(e) = c.connect() {
            tracing::debug!(error = %e, "discord is not running; presence not shown");
            return false;
        }
        *client = Some(c);
    }
    let Some(c) = client.as_mut() else { return false };
    match c.set_activity(to_activity(&parts)) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, "discord: setting the presence failed; reconnecting later");
            if let Some(mut c) = client.take() {
                let _ = c.close();
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watching(paused: bool) -> PresenceVm {
        PresenceVm::Watching {
            title: "Frieren".into(),
            episode: 3,
            position_secs: 60,
            duration_secs: Some(1440),
            paused,
            poster: Some("https://img.example/frieren.jpg".into()),
        }
    }

    #[test]
    fn clear_shows_nothing_and_browsing_uses_the_logo() {
        assert_eq!(activity_parts(&PresenceVm::Clear, 0), None);
        let b = activity_parts(&PresenceVm::Browsing, 0).unwrap();
        assert_eq!(b.large_image, LOGO_KEY);
        assert_eq!((b.start_ms, b.end_ms, b.watching), (None, None, false));
        let v = activity_parts(&PresenceVm::Viewing { title: "Frieren".into() }, 0).unwrap();
        assert!(v.details.contains("Frieren"));
    }

    #[test]
    fn watching_has_the_poster_and_the_remaining_time() {
        let now = 1_000_000;
        let w = activity_parts(&watching(false), now).unwrap();
        assert!(w.details.contains("Frieren"));
        assert!(w.state.contains('3'));
        assert_eq!(w.large_image, "https://img.example/frieren.jpg");
        assert_eq!(w.start_ms, Some(now - 60_000));
        assert_eq!(w.end_ms, Some(now + (1440 - 60) * 1000));
        assert_eq!(w.small_image, Some(PLAY_KEY));
        assert!(w.watching);
    }

    #[test]
    fn paused_has_no_time_bar() {
        let p = activity_parts(&watching(true), 5).unwrap();
        assert_eq!((p.start_ms, p.end_ms), (None, None));
        assert_eq!(p.small_image, Some(PAUSE_KEY));
        assert_ne!(p.details, activity_parts(&watching(false), 5).unwrap().details);
    }

    #[test]
    fn texts_fit_discord_limits_and_only_https_posters_are_used() {
        let long = "x".repeat(300);
        let w = PresenceVm::Watching {
            title: long.clone(),
            episode: 1,
            position_secs: 0,
            duration_secs: None,
            paused: false,
            poster: Some("http://insecure/p.jpg".into()),
        };
        let p = activity_parts(&w, 0).unwrap();
        assert!(p.details.chars().count() <= MAX_TEXT);
        assert!(p.large_text.chars().count() <= MAX_TEXT);
        assert_eq!(p.large_image, LOGO_KEY);
        assert_eq!(p.end_ms, None, "unknown duration: elapsed only");
        assert_eq!(fit(String::new()).chars().count(), 2);
    }
}
